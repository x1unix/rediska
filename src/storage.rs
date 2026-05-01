use bytes::Bytes;
use std::collections::HashMap;
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DBError {
    #[error("operation against a key holding the wrong kind of value")]
    WrongKeyType,
}

/// Describes value stored in DB.
pub enum Value {
    String(Bytes),
    List(Vec<Bytes>),
}

pub struct Entry {
    pub value: Bytes,
    pub expire_at: Option<Duration>,
}

impl Entry {
    pub fn ttl_is_before(&self, now: Duration) -> bool {
        match self.expire_at {
            Some(ttl) => ttl > now,
            None => true,
        }
    }
}

pub struct MemDB {
    kv: HashMap<Bytes, Entry>,
}

impl MemDB {
    pub fn new() -> Self {
        Self { kv: HashMap::new() }
    }

    pub fn get(&self, key: &Bytes) -> Option<&Entry> {
        // let now = SystemTime::now().duration_since(UNIX_EPOCH);
        // self.kv.get(key).map(|e| e.value.to_owned())
        self.kv.get(key)
    }

    pub fn set(&mut self, key: &Bytes, val: &Bytes, expire_at: Option<Duration>) {
        // key and val pointing to memory area with parsed request.
        // copy to avoid mem leak.
        let key = Bytes::copy_from_slice(key.as_ref());
        let value = Bytes::copy_from_slice(val.as_ref());
        self.kv.insert(key, Entry { value, expire_at });
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
