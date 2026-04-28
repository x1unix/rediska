use anyhow::Context;
use bytes::{BufMut, Bytes, BytesMut};
use std::collections::HashMap;
use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{self, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

use crate::reader::{ReadError, StreamParser};
use crate::request::{Request, RequestError, TTL};

pub struct Entry {
    value: Bytes,
    expire_at: Option<Duration>,
}

pub struct Storage {
    kv: HashMap<Bytes, Entry>,
}

impl Storage {
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
}

impl Default for Storage {
    fn default() -> Self {
        Self::new()
    }
}

type SyncStorage = Arc<Mutex<Storage>>;

/// Starts Redis listener on a given address.
pub async fn listen(addr: &str) -> Result<(), io::Error> {
    let listener = TcpListener::bind(addr).await?;
    println!("Listening on {addr}");

    // TODO: use RWLock
    let db = Arc::new(Mutex::new(Storage::new()));
    loop {
        let (sock, addr) = listener.accept().await?;
        let db = db.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_conn(addr, sock, db.clone()).await {
                println!("Error: {e}");
            }
        });
    }
}

// TODO: use traits instead
pub async fn handle_conn(
    addr: SocketAddr,
    mut s: TcpStream,
    db: SyncStorage,
) -> anyhow::Result<()> {
    println!("Conn: {addr}");
    loop {
        match read_request(&mut s).await {
            Ok(Some(req)) => handle_req(&mut s, req, db.clone())
                .await
                .unwrap_or_else(|e| {
                    println!("Err: can't write response: {e:?}");
                }),
            Ok(None) => break,
            Err(RequestError::ReadError(ReadError::Io(err))) => {
                println!("IO Error: {err:?}");
                break;
            }
            Err(err) => {
                println!("Err: {err:?}");
                dump_err(&mut s, err).await;
                break;
            }
        }
    }

    Ok(())
}

const RSP_OK: &[u8] = b"+OK\r\n";
const RSP_NUL_STR: &[u8] = b"$-1\r\n";

fn str_response(msg: &Bytes) -> Bytes {
    let mut out = BytesMut::new();
    out.put_u8(b'$');
    out.extend_from_slice(msg.len().to_string().as_bytes());
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(msg);
    out.extend_from_slice(b"\r\n");
    out.freeze()
}

async fn handle_req(s: &mut TcpStream, req: Request, db: SyncStorage) -> anyhow::Result<()> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("unable to get system timestamp")?;

    // TODO: proper response builder
    let rsp = match req {
        Request::Ping => Bytes::from_static(b"+PONG\r\n"),
        Request::Echo { msg } => str_response(&msg),
        Request::Get { key } => db
            .lock()
            .await
            .get(&key)
            .filter(|v| v.expire_at.map(|ttl| ttl > now).unwrap_or(true))
            .map(|v| str_response(&v.value))
            .unwrap_or_else(|| Bytes::from_static(RSP_NUL_STR)),
        Request::Set { key, val, ttl } => {
            let ttl = ttl.map(|v| v.as_unix()).transpose()?;
            db.lock().await.set(&key, &val, ttl);
            Bytes::from_static(RSP_OK)
        }
    };

    s.write_all(&rsp).await?;
    Ok(())
}

async fn dump_err<E: fmt::Display>(s: &mut TcpStream, err: E) {
    // TODO: make this in proper way
    let msg = err.to_string().replace('\r', "\\r").replace('\n', "\\n");
    if let Err(err) = s.write_all(format!("-ERR {msg}\r\n").as_bytes()).await {
        println!("Err: can't write response: {err:?}");
    }
}

async fn read_request(s: &mut TcpStream) -> Result<Option<Request>, RequestError> {
    let mut parser = StreamParser::new(s, 1024);
    let Some(val) = parser.parse().await? else {
        return Ok(None);
    };

    let req: Request = val.try_into()?;
    Ok(Some(req))
}
