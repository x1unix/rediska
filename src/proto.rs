use std::usize;

use bytes::BytesMut;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use anyhow::{Context, Result, anyhow};

pub enum Value {
    Array(Vec<Value>),
    BulkString(String),
}

pub async fn read_stream<T>(s: &mut T) -> Result<Option<Value>>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    // TODO: read all
    // let mut buff: [u8; 4] = [0; 4];
    let mut buff: [u8; 32] = [0; 32];
    let n = s.read(&mut buff[..]).await?;
    if n == 0 {
        return Ok(None);
    }

    let req = str::from_utf8(&buff[..n]).context("payload is not a text")?;
    println!("Req: {:?}", req);
    // let response = match req {
    //     "PING" => "+PONG\r\n",
    //     _ => return Err(anyhow!("invalid cmd: {}", req)),
    // };
    let response = "+PONG\r\n";

    s.write_all(response.as_bytes())
        .await
        .context("can't write response")?;

    // Ok(Some(()))
    Ok(None)
}

enum FrameKind {
    Array { len: u32 },
    BulkString { len: u32 },
    NullBulkString,
    Delimiter,
    Literal,
}

#[derive(Debug)]
struct BufRef(usize, usize);

impl BufRef {
    fn offset(&self) -> usize {
        self.0
    }

    fn len(&self) -> usize {
        self.1
    }
}

#[derive(Debug, Error)]
enum ParseError {
    #[error("end of buffer")]
    EOF,

    #[error("end of buffer")]
    BadFrame(BufRef),

    #[error("invalid length value")]
    BadLength(BufRef),
}

fn parse_frame(src: &[u8]) -> Result<(FrameKind, usize), ParseError> {
    if src.is_empty() {
        return Err(ParseError::EOF);
    }

    return match src[0] {
        b'\r' => match src.get(1) {
            None => Err(ParseError::EOF),
            Some(v) if *v == b'\n' => Ok((FrameKind::Delimiter, 2)),
            _ => Err(ParseError::BadFrame(BufRef(0, 2))),
        },
        b'*' => {
            todo!()
        }
        _ => todo!(),
    };

    todo!()
}

enum LookupResult<T> {
    Ok(T),
    None,
    EOF,
}

/// Reads a given buffer from offset and reads an integer value till carriage return character (\n).
/// Returns read value and offset after a numeric string.
fn read_num(src: &[u8], offset: usize) -> Result<(i64, usize), ParseError> {
    let is_neg = match src.get(offset) {
        None => return Err(ParseError::EOF),
        Some(b'-') => true,
        _ => false,
    };

    let mut is_empty = true;
    let mut acc: i64 = 0;
    let mut i = if is_neg { offset + 1 } else { offset };
    while i < src.len() {
        match src.get(i) {
            Some(b'\r') => {
                // End of frame
                return if is_empty {
                    Err(ParseError::BadLength(BufRef(offset, i - offset)))
                } else {
                    Ok((acc, i))
                };
            }
            Some(v @ b'0'..=b'9') => {
                is_empty = false;
                let d = (v - b'0') as i64;
                acc = acc
                    .checked_mul(10)
                    .and_then(|v| {
                        if is_neg {
                            v.checked_sub(d)
                        } else {
                            v.checked_add(d)
                        }
                    })
                    .ok_or(ParseError::BadLength(BufRef(offset, i - offset)))?;
                i += 1;
            }
            Some(_) => return Err(ParseError::BadLength(BufRef(offset, i - offset))),
            None => return Err(ParseError::EOF),
        }
    }

    // need more data to read till delimiter
    Err(ParseError::EOF)
}

fn seek_eol(src: &[u8], offset: usize) -> LookupResult<usize> {
    // NOTE: redis-cli split strings into separate frames only by "\n". "foo\rbar" is single frame.
    match (src.get(offset), src.get(offset + 1)) {
        (Some(b'\r'), Some(b'\n')) => LookupResult::Ok(offset + 2),
        (_, None) => LookupResult::EOF,
        _ => LookupResult::None,
    }
}

// async fn read_frame<T>(src: &mut T, buff: &mut BytesMut) -> Result<Option<ReadResult>>
// where
//     T: AsyncRead + AsyncWrite + Unpin,
// {
//     let n = src.read(buff).await?;
//     if n == 0 {
//         return Ok(None);
//     }
//
//     let segment = &buff[..n];
//
//     for ch in segment {
//         todo!();
//     }
//
//     todo!()
// }
