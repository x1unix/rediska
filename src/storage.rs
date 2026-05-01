use bytes::Bytes;
use std::collections::HashMap;
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DBError {
    #[error("WRONGTYPE operation against a key holding the wrong kind of value")]
    WrongType,
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
        self.kv.get(key)
    }

    pub fn set(&mut self, key: &Bytes, entry: Entry) {
        // key is pointing to memory area with parsed request.
        // copy to avoid mem leak.
        let key = Bytes::copy_from_slice(key.as_ref());
        self.kv.insert(key, entry);
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
