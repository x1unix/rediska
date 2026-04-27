use bytes::{BufMut, BytesMut};
use std::fmt;
use std::net::SocketAddr;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use crate::reader::{ReadError, StreamParser};
use crate::request::{Request, RequestError};

// TODO: use traits instead
pub async fn handle_conn(addr: SocketAddr, mut s: TcpStream) -> anyhow::Result<()> {
    println!("Conn: {addr}");
    loop {
        match read_request(&mut s).await {
            Ok(Some(req)) => respond(&mut s, req).await.unwrap_or_else(|e| {
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

async fn respond(s: &mut TcpStream, req: Request) -> anyhow::Result<()> {
    // TODO: proper response builder
    match req {
        Request::Ping => s.write_all(b"+PONG\r\n").await?,
        Request::Echo { msg } => {
            let mut out = BytesMut::new();
            out.put_u8(b'$');
            out.extend_from_slice(msg.len().to_string().as_bytes());
            out.extend_from_slice(b"\r\n");
            out.extend_from_slice(&msg);
            out.extend_from_slice(b"\r\n");
            s.write_all(&out).await?
        }
    };

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
