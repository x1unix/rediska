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
use crate::storage::{Entry, Keyspace, MemDB, Value};

/// Starts Redis listener on a given address.
pub async fn listen(addr: &str) -> Result<(), io::Error> {
    let listener = TcpListener::bind(addr).await?;
    println!("Listening on {addr}");

    // TODO: use RWLock
    // let db = Arc::new(Mutex::new(MemDB::new()));
    let db = Arc::new(Keyspace::default());
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
    db: Arc<Keyspace>,
) -> anyhow::Result<()> {
    println!("Conn: {addr}");
    let mut buf = BytesMut::with_capacity(1024);
    loop {
        match read_request(&mut buf, &mut s).await {
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

// TODO: use error types
fn str_response(msg: &Bytes) -> Bytes {
    let mut out = BytesMut::new();
    out.put_u8(b'$');
    out.extend_from_slice(msg.len().to_string().as_bytes());
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(msg);
    out.extend_from_slice(b"\r\n");
    out.freeze()
}

async fn handle_req(s: &mut TcpStream, req: Request, db: Arc<Keyspace>) -> anyhow::Result<()> {
    // TODO: proper response builder + staleness checker
    let rsp = match req {
        Request::Ping => Bytes::from_static(b"+PONG\r\n"),
        Request::Echo { msg } => str_response(&msg),
        Request::Get { key } => db
            .scalar_get(&key)
            .await
            .map(|r| match r {
                Some(b) => str_response(&b),
                None => Bytes::from_static(RSP_NUL_STR),
            })
            .unwrap_or_else(|e| e.as_resp_bytes()),
        // Request::Get { key } => match db.scalar_get(&key).await? {
        //     Some(b) => str_response(&b),
        //     None => Bytes::from_static(RSP_NUL_STR),
        // },
        Request::Set { key, val, ttl } => {
            let ttl = ttl.map(|v| v.as_unix()).transpose()?;
            db.scalar_set(&key, &val, ttl)
                .await
                .map(|_| Bytes::from_static(RSP_OK))
                .unwrap_or_else(|e| e.as_resp_bytes())
            // Bytes::from_static(RSP_OK)
        }
        Request::Rpush { key, values } => db
            .list_push(&key, values)
            .await
            .map(|n| Bytes::from(format!(":{n}\r\n")))
            .unwrap_or_else(|e| e.as_resp_bytes()),
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

async fn read_request(
    buf: &mut BytesMut,
    s: &mut TcpStream,
) -> Result<Option<Request>, RequestError> {
    let mut parser = StreamParser::new(s, buf);
    let Some(val) = parser.parse().await? else {
        return Ok(None);
    };

    let req: Request = val.try_into()?;
    Ok(Some(req))
}
