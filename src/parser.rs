use bytes::BytesMut;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::proto::{BufRef, FrameKind, ParseError, WordRef, parse_frame, read_word};

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
    consumed_bytes: usize,
    refs: Vec<ValueRef>,
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
            consumed_bytes: 0,
        }
    }

    pub async fn parse(&mut self) -> Result<Option<()>, ParseError> {
        // Two stage parser.
        // Stage 1: consume reader into a buffer + parse frames into a flat ref tree.
        // Stage 2: freeze the buff and build a tree with dereferenced values.

        let mut segments: Vec<ValueRef> = Vec::with_capacity(5);
        while let Some(seg) = self.frame_with_eol().await? {
            match seg {
                FrameKind::Array { len, .. } => segments.push(ValueRef::Array { len }),
                FrameKind::BulkString { len, .. } => {
                    // TODO: read string after it
                    segments.push(ValueRef::String(BufRef::new(self.pos, 0)));
                }
                _ => {
                    return Err(ParseError::UnexpectedFrame {
                        offset: self.pos - seg.byte_len(),
                        frame: seg,
                    });
                }
            }
        }

        todo!()
    }

    async fn read_word(&mut self) -> Result<WordRef, ParseError> {
        loop {
            match read_word(&self.buf, self.pos) {
                Ok(Some(word)) => {
                    self.pos = word.end;
                    return Ok(word);
                }
                Ok(None) => return Err(ParseError::Incomplete),
                Err(ParseError::Incomplete) => {
                    let n = self.reader.read_buf(&mut self.buf).await?;
                    if n == 0 {
                        return Err(ParseError::Incomplete);
                    }
                }
                Err(e) => return Err(e),
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
