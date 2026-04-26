use bytes::{Bytes, BytesMut};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::proto::{BufRef, FrameKind, ParseError, check_word, parse_frame};

enum Value {
    String(Bytes),
    Array(Vec<Value>),
}

enum ValueRef {
    Array { len: u64 },
    String(BufRef),
    Empty,
    Nil,
}

pub struct StreamParser<T: AsyncRead + Unpin> {
    reader: T,
    buf: BytesMut,
    pos: usize,
    total_bytes: usize, // global offset
    refs: Vec<ValueRef>,
}

enum Container {
    Array {
        remaining: usize,
        items: Vec<Value>,
    },
    Map {
        remaining: usize,
        entries: Vec<(Value, Value)>,
        pending_key: Option<Value>,
    },
}

impl Container {
    pub fn new_array(count: u64) -> Self {
        return Self::Array {
            remaining: count as usize,
            items: Vec::with_capacity(count as usize),
        };
    }
}

impl<T> StreamParser<T>
where
    T: AsyncRead + Unpin,
{
    pub fn new(src: T, buff_size: usize) -> Self {
        Self {
            reader: src,
            buf: BytesMut::with_capacity(buff_size),
            refs: Vec::with_capacity(4),
            pos: 0,
            total_bytes: 0,
        }
    }

    fn split_buf(&mut self) -> Bytes {
        let b = self.buf.split_to(self.pos).freeze();
        self.total_bytes += self.pos;
        self.pos = 0;
        b
    }

    pub async fn parse(&mut self) -> Result<Option<Value>, ParseError> {
        let Some(root) = self.frame_with_eol().await? else {
            return Ok(None);
        };

        let mut stack: Vec<Container> = Vec::new();
        match root {
            FrameKind::BulkString { len, .. } => {
                let w = self.read_word(len as usize).await?;
                let raw = self.split_buf();
                let data = raw.slice(w.offset()..w.end());
                return Ok(Some(Value::String(data)));
            }
            FrameKind::Array { len, .. } => {
                stack.push(Container::new_array(len));
            }
            _ => {
                return Err(ParseError::UnexpectedFrame {
                    offset: self.pos - root.byte_len(),
                    frame: root,
                });
            }
        }

        while stack.len() > 0 {
            let tail = stack.last_mut()
        }

        todo!()
    }

    // pub async fn parse(&mut self) -> Result<Option<Value>, ParseError> {
    //     let Some(root) = self.frame_with_eol().await? else {
    //         return Ok(None);
    //     };
    //
    //     match root {
    //         FrameKind::BulkString { len, .. } => {
    //             let w = self.read_word(len as usize).await?;
    //             let raw = self.split_buf();
    //             let data = raw.slice(w.offset()..w.end());
    //             return Ok(Some(Value::String(data)));
    //         }
    //         FrameKind::Array { len, .. } => {
    //             // Discard behind
    //             self.split_buf();
    //
    //             // Collect all frames
    //             self.read_array(len as usize).await?;
    //             // segments.push(ValueRef::Array { len });
    //         }
    //         _ => {
    //             return Err(ParseError::UnexpectedFrame {
    //                 offset: self.pos - root.byte_len(),
    //                 frame: root,
    //             });
    //         }
    //     }
    //     todo!()
    // }
    //
    // async fn read_array(&mut self, count: usize) -> Result<(), ParseError> {
    //     // TODO: support other types
    //     let mut stack: Vec<Container> = vec![Container::new_array(count)];
    //     // let mut acc: Vec<ValueRef> = Vec::with_capacity((count + 1) as usize);
    //     // acc.push(ValueRef::Array { len: count });
    //
    //     // while stack.len() > 0 {
    //     //
    //     // }
    //     let current = Container::new_array(count);
    //     for _ in 0..count {
    //         let frame = self.frame_with_eol().await?.ok_or(ParseError::Incomplete)?;
    //         match frame {
    //             FrameKind::Array { len, .. } => {
    //                 stack.push(value);
    //                 acc.push(ValueRef::Array { len });
    //             }
    //             FrameKind::BulkString { len, .. } => {
    //                 acc.push(ValueRef::String(BufRef::new(self.pos, len as usize)));
    //             }
    //             _ => {
    //                 todo!()
    //             }
    //         }
    //     }
    //
    //     let raw = self.split_buf();
    //
    //     todo!()
    // }

    async fn read_word(&mut self, len: usize) -> Result<BufRef, ParseError> {
        loop {
            match check_word(&self.buf, self.pos, len) {
                Ok(word) => {
                    self.pos += len + 2;
                    return Ok(word);
                }
                Err(ParseError::Incomplete) => {
                    let n = self.reader.read_buf(&mut self.buf).await?;
                    if n == 0 {
                        return Err(ParseError::Incomplete);
                    }
                }
                Err(err) => return Err(err),
            }
        }
    }

    // async fn read_ref(&mut self) -> Result<Option<ValueRef>, ParseError> {
    //     let Some(f) = self.frame_with_eol().await? else {
    //         return Ok(None);
    //     };
    //
    //     match f {
    //         FrameKind::Array { len, .. } => {
    //             for _ in 0..len {
    //                 let elem = self.read_ref().await?.ok_or(ParseError::Incomplete);
    //             }
    //         }
    //         _ => todo!(),
    //     }
    //     todo!()
    // }

    /// Reads the next frame + consumes EOL followed by the frame.
    async fn frame_with_eol(&mut self) -> Result<Option<FrameKind>, ParseError> {
        let f = match self.next_frame().await? {
            Some(FrameKind::Delimiter) => {
                return Err(ParseError::UnexpectedFrame {
                    offset: self.pos,
                    frame: FrameKind::Delimiter,
                });
            }
            Some(f) => f,
            None => return Ok(None),
        };

        self.consume_eol().await?;
        Ok(Some(f))
    }

    async fn consume_eol(&mut self) -> Result<(), ParseError> {
        let offset = self.pos;
        match self.next_frame().await? {
            None => Err(ParseError::Incomplete),
            Some(FrameKind::Delimiter) => Ok(()),
            Some(f) => Err(ParseError::UnexpectedFrame { offset, frame: f }),
        }
    }

    async fn next_frame(&mut self) -> Result<Option<FrameKind>, ParseError> {
        loop {
            match parse_frame(&self.buf, self.pos) {
                Ok(Some((frame, next))) => {
                    self.pos = next;
                    return Ok(Some(frame));
                }
                Ok(None) => {
                    // Fill buffer and retry
                    let n = self.reader.read_buf(&mut self.buf).await?;
                    if n == 0 {
                        return Ok(None);
                    }
                }
                Err(ParseError::Incomplete) => {
                    // Incomplete, ask more bytes
                    let n = self.reader.read_buf(&mut self.buf).await?;
                    if n == 0 {
                        return Err(ParseError::Incomplete);
                    }
                }
                Err(e) => return Err(e),
            }
        }
    }
}
