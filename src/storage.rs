use bytes::Bytes;
use std::collections::HashMap;
use std::collections::hash_map::Entry as HashEntry;
use std::sync::Arc;
use std::time::Duration;
use std::time::{SystemTime, SystemTimeError, UNIX_EPOCH};
use thiserror::Error;
use tokio::sync::Mutex;

#[derive(Debug, Error)]
pub enum KeyError {
    #[error("operation against a key holding the wrong kind of value")]
    WrongType,

    #[error("system type error")]
    SystemTimeError(#[from] SystemTimeError),
}

impl KeyError {
    pub fn code(&self) -> &'static str {
        match self {
            KeyError::WrongType => "WRONGTYPE",
            _ => "ERR",
        }
    }
}

/// Describes value stored in DB.
pub enum Value {
    String(Bytes),
    List(Vec<Bytes>),
}

pub struct Entry {
    pub value: Value,
    pub expire_at: Option<Duration>,
}

impl Entry {
    pub fn new_string(val: &Bytes, expire_at: Option<Duration>) -> Self {
        // value pointing to memory area with parsed request.
        // copy to avoid mem leak.
        let b = Bytes::copy_from_slice(val.as_ref());
        Self {
            value: Value::String(b),
            expire_at,
        }
    }

    pub fn new_list(entries: Vec<Bytes>) -> Self {
        Self {
            value: Value::List(entries),
            expire_at: None,
        }
    }

    pub fn ttl_is_before(&self, now: Duration) -> bool {
        match self.expire_at {
            Some(ttl) => ttl > now,
            None => true,
        }
    }
}

pub enum InsertOrder {
    Append,
    Prepend,
}

impl InsertOrder {
    fn apply_list(self, dst: &mut Vec<Bytes>, src: Vec<Bytes>) {
        match self {
            Self::Append => {
                dst.extend(src);
            }
            Self::Prepend => {
                for e in src {
                    dst.insert(0, e);
                }
            }
        }
    }
}

pub enum PopOrder {
    Start(usize),
    End(usize),
}

impl PopOrder {
    fn apply(self, dst: &mut Vec<Bytes>) -> Vec<Bytes> {
        match self {
            Self::Start(n) => {
                let n = n.clamp(0, dst.len());
                dst.drain(0..n).collect()
            }
            Self::End(n) => {
                let n = n.clamp(0, dst.len());
                dst.split_off(n)
            }
        }
    }
}

pub struct Keyspace {
    db: Mutex<MemDB>,
}

impl Keyspace {
    pub fn default() -> Self {
        let db = Mutex::new(MemDB::new());
        Self { db }
    }

    pub async fn scalar_get(&self, key: &Bytes) -> Result<Option<Bytes>, KeyError> {
        let now = get_now()?;
        let db = self.db.lock().await;
        db.get(key)
            .filter(|v| v.ttl_is_before(now))
            .map(|v| match v.value {
                Value::String(ref b) => Ok(b.to_owned()),
                _ => Err(KeyError::WrongType),
            })
            .transpose()
    }

    pub async fn scalar_set(
        &self,
        key: &Bytes,
        val: &Bytes,
        expire_at: Option<Duration>,
    ) -> Result<(), KeyError> {
        let now = get_now()?;
        let mut db = self.db.lock().await;

        // Validate key type
        db.get(key)
            .filter(|e| e.ttl_is_before(now))
            .map(|v| match v.value {
                Value::String(_) => Ok(()),
                _ => Err(KeyError::WrongType),
            })
            .transpose()?;

        db.set(key, Entry::new_string(val, expire_at));
        Ok(())
    }

    pub async fn list_pop(
        &self,
        key: &Bytes,
        order: PopOrder,
    ) -> Result<Option<Vec<Bytes>>, KeyError> {
        let now = get_now()?;
        let mut db = self.db.lock().await;
        match db.entry(key) {
            HashEntry::Occupied(mut slot) => {
                if slot.get().ttl_is_before(now) {
                    match &mut slot.get_mut().value {
                        Value::List(arr) => {
                            let out = order.apply(arr);
                            Ok(Some(out))
                        }
                        _ => Err(KeyError::WrongType),
                    }
                } else {
                    Ok(None)
                }
            }
            _ => Ok(None),
        }
    }

    pub async fn list_insert(
        &self,
        key: &Bytes,
        entries: Vec<Bytes>,
        order: InsertOrder,
    ) -> Result<usize, KeyError> {
        let now = get_now()?;
        let mut db = self.db.lock().await;
        match db.entry(key) {
            HashEntry::Occupied(mut slot) => {
                if slot.get().ttl_is_before(now) {
                    match &mut slot.get_mut().value {
                        Value::List(arr) => {
                            order.apply_list(arr, entries);
                            // arr.extend(entries);
                            Ok(arr.len())
                        }
                        _ => Err(KeyError::WrongType),
                    }
                } else {
                    let len = entries.len();
                    slot.insert(Entry::new_list(entries));
                    Ok(len)
                }
            }
            HashEntry::Vacant(slot) => {
                let len = entries.len();
                slot.insert(Entry::new_list(entries));
                Ok(len)
            }
        }
    }

    pub async fn list_range(
        &self,
        key: &Bytes,
        start: i32,
        end: i32,
    ) -> Result<Option<Vec<Bytes>>, KeyError> {
        let now = get_now()?;
        let db = self.db.lock().await;
        let e = db
            .get(key)
            .filter(|v| v.ttl_is_before(now))
            .map(|e| &e.value);

        match e {
            Some(Value::List(l)) => {
                // TODO
                if let Some((start, end)) = convert_range(start, end, l.len()) {
                    // TODO: RefCell?
                    let r = &l[start..=end];
                    Ok(Some(r.to_vec()))
                } else {
                    Ok(None)
                }
            }
            Some(_) => Err(KeyError::WrongType),
            None => Ok(None),
        }
    }

    pub async fn list_len(&self, key: &Bytes) -> Result<Option<usize>, KeyError> {
        let now = get_now()?;
        let db = self.db.lock().await;
        db.get(key)
            .filter(|v| v.ttl_is_before(now))
            .map(|e| {
                if let Value::List(l) = &e.value {
                    Ok(l.len())
                } else {
                    Err(KeyError::WrongType)
                }
            })
            .transpose()
    }
}

fn convert_range(start: i32, end: i32, len: usize) -> Option<(usize, usize)> {
    if len == 0 {
        return None;
    }

    let start = index_from_pos(start, len);
    let end = index_from_pos(end, len);
    if start > end {
        None
    } else {
        Some((start, end))
    }
}

fn index_from_pos(pos: i32, len: usize) -> usize {
    if len == 0 {
        return 0;
    };

    let ilen = len as i32;
    let last_idx = ilen - 1;

    let i = if pos < 0 { ilen + pos } else { pos };

    i.clamp(0, last_idx) as usize
}

fn get_now() -> Result<Duration, KeyError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.into())
}

// TODO: separate key types into separate shards.

pub struct MemDB {
    kv: HashMap<Bytes, Entry>,
}

impl MemDB {
    pub fn new() -> Self {
        Self { kv: HashMap::new() }
    }

    pub fn get(&self, key: &Bytes) -> Option<&Entry> {
        self.kv.get(key)
    }

    pub fn set(&mut self, key: &Bytes, entry: Entry) {
        // key is pointing to memory area with parsed request.
        // copy to avoid mem leak.
        let key = Bytes::copy_from_slice(key.as_ref());
        self.kv.insert(key, entry);
    }

    pub fn entry(&mut self, key: &Bytes) -> HashEntry<'_, Bytes, Entry> {
        self.kv.entry(Bytes::copy_from_slice(key.as_ref()))
    }

    pub fn del(&mut self, key: &Bytes) -> Option<Entry> {
        self.kv.remove(key)
    }
}

impl Default for MemDB {
    fn default() -> Self {
        Self::new()
    }
}
