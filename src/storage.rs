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

    pub fn as_resp_bytes(&self) -> Bytes {
        match self {
            KeyError::WrongType => Bytes::from_static(
                b"-WRONGTYPE operation against a key holding the wrong kind of value\r\n",
            ),
            KeyError::SystemTimeError(err) => {
                Bytes::from(format!("-ERR system time error: {err}\r\n"))
            }
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

    pub async fn list_push(&self, key: &Bytes, entries: Vec<Bytes>) -> Result<usize, KeyError> {
        let now = get_now()?;
        let mut db = self.db.lock().await;
        match db.entry(key) {
            HashEntry::Occupied(mut slot) => {
                if slot.get().ttl_is_before(now) {
                    match &mut slot.get_mut().value {
                        Value::List(arr) => {
                            arr.extend(entries);
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
}

fn get_now() -> Result<Duration, KeyError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.into())
}

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
