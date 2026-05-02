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
use crate::request::{Request, RequestError, Ttl};
use crate::response::BufferBuilder;
use crate::storage::{Entry, InsertOrder, KeyError, Keyspace, MemDB, PopOrder, Value};

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
    let mut rsp = BufferBuilder::default();

    loop {
        match read_request(&mut buf, &mut s).await {
            Ok(Some(req)) => {
                if let Err(err) = handle_req(&mut rsp, req, db.clone()).await {
                    rsp.err(Some(err.code()), err.to_string().as_ref());
                    println!("Err: {err:?}");
                }

                let body = rsp.build();
                // println!("Resp: {body:?}");
                s.write_all(body.as_ref()).await?;
            }
            Ok(None) => break,
            Err(RequestError::ReadError(ReadError::Io(err))) => {
                println!("IO Error: {err:?}");
                break;
            }
            Err(err) => {
                println!("Err: {err:?}");
                rsp.err(None, err.to_string().as_ref());
                s.write_all(rsp.build().as_ref()).await?;
                break;
            }
        }
    }

    Ok(())
}

async fn handle_req(
    rsp: &mut BufferBuilder,
    req: Request,
    db: Arc<Keyspace>,
) -> Result<(), KeyError> {
    // TODO: staleness checker
    match req {
        Request::Ping => {
            rsp.pong();
        }
        Request::Echo { msg } => {
            rsp.str_bulk(msg.as_ref());
        }
        Request::Get { key } => match db.scalar_get(&key).await? {
            Some(b) => {
                rsp.str_bulk(b.as_ref());
            }
            None => {
                rsp.null_bulk_str();
            }
        },
        Request::Set { key, val, ttl } => {
            let ttl = ttl.map(|v| v.as_unix()).transpose()?;
            db.scalar_set(&key, &val, ttl).await?;
            rsp.ok();
        }
        Request::Llen { key } => {
            let n = db.list_len(&key).await?.unwrap_or(0);
            rsp.integer(n);
        }
        Request::Rpush { key, values } => {
            let n = db.list_insert(&key, values, InsertOrder::Append).await?;
            rsp.integer(n);
        }
        Request::Lpush { key, values } => {
            let n = db.list_insert(&key, values, InsertOrder::Prepend).await?;
            rsp.integer(n);
        }
        Request::Lrange { key, start, end } => {
            let parts = db.list_range(&key, start, end).await?;
            if let Some(parts) = parts {
                rsp.array(parts.len());
                parts.iter().for_each(|e| {
                    rsp.str_bulk(e.as_ref());
                });
            } else {
                rsp.array(0);
            }
        }
        Request::Lpop { key, n } => {
            let out = db.list_pop(&key, PopOrder::Start(n)).await?;

            if let Some(parts) = out {
                rsp.array(parts.len());
                parts.iter().for_each(|e| {
                    rsp.str_bulk(e.as_ref());
                });
            } else {
                rsp.array(0);
            }
        }
    };

    Ok(())
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
