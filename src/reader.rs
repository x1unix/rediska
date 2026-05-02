use bytes::{Bytes, BytesMut};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::assembler::{AssembleError, Value, ValueRef, assemble};
use crate::parser::{BufRef, FrameKind, ParseError, check_word, parse_frame};

#[derive(Debug, Error)]
pub enum ReadError {
    #[error("missing frame")]
    MissingFrame { frame_count: u64 },

    #[error("parse error")]
    ParseError(#[from] ParseError),

    #[error("assemble error")]
    AssembleError(#[from] AssembleError),

    #[error("io error")]
    Io(#[from] std::io::Error),
}

pub struct StreamParser<'a, T: AsyncRead + Unpin> {
    reader: T,
    buf: &'a mut BytesMut,
    pos: usize,
    total_bytes: usize, // global offset
}

impl<'a, T> StreamParser<'a, T>
where
    T: AsyncRead + Unpin,
{
    pub fn new(src: T, buf: &'a mut BytesMut) -> Self {
        Self {
            reader: src,
            buf,
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

    pub async fn parse(&mut self) -> Result<Option<Value>, ReadError> {
        let Some(refs) = self.collect_stream().await? else {
            return Ok(None);
        };

        let buf = self.split_buf();
        let items = assemble(buf, &refs[..])?;
        Ok(items)
    }

    async fn collect_stream(&mut self) -> Result<Option<Vec<ValueRef>>, ReadError> {
        let mut pending_items: u64 = 1;
        let mut items = Vec::<ValueRef>::with_capacity(2);
        while pending_items > 0 {
            let Some(f) = self.frame_with_eol().await? else {
                return if items.is_empty() {
                    // Request is empty
                    Ok(None)
                } else {
                    Err(ReadError::MissingFrame {
                        frame_count: pending_items,
                    })
                };
            };

            match f {
                FrameKind::BulkString { len, .. } => {
                    let w = self.read_word(len as usize).await?;
                    items.push(ValueRef::String(w));
                    pending_items -= 1;
                }
                FrameKind::Array { len, .. } => {
                    items.push(ValueRef::Array { len });
                    pending_items -= 1;
                    pending_items += len;
                }
                _ => todo!("support more frame types"),
            }
        }

        Ok(Some(items))
    }

    async fn read_word(&mut self, len: usize) -> Result<BufRef, ReadError> {
        loop {
            match check_word(self.buf, self.pos, len) {
                Ok(word) => {
                    // let prev_pos = self.pos;
                    // let next_pos = self.pos + len + 2;
                    // println!("fword: i={prev_pos}, next={next_pos}, v={word:?}");
                    self.pos += len + 2;
                    return Ok(word);
                }
                Err(ParseError::IncompleteBuffer) => {
                    let n = self.reader.read_buf(&mut self.buf).await?;
                    if n == 0 {
                        return Err(ReadError::ParseError(ParseError::IncompleteBuffer));
                    }
                }
                Err(err) => return Err(err.into()),
            }
        }
    }

    /// Reads the next frame + consumes EOL followed by the frame.
    async fn frame_with_eol(&mut self) -> Result<Option<FrameKind>, ReadError> {
        let f = match self.next_frame().await? {
            Some(FrameKind::Delimiter) => {
                return Err(ParseError::UnexpectedFrame {
                    offset: self.pos,
                    frame: FrameKind::Delimiter,
                }
                .into());
            }
            Some(f) => f,
            None => return Ok(None),
        };

        self.consume_eol().await?;
        Ok(Some(f))
    }

    async fn consume_eol(&mut self) -> Result<(), ReadError> {
        let offset = self.pos;
        match self.next_frame().await? {
            None => Err(ParseError::IncompleteBuffer.into()),
            Some(FrameKind::Delimiter) => Ok(()),
            Some(f) => Err(ParseError::UnexpectedFrame { offset, frame: f }.into()),
        }
    }

    async fn next_frame(&mut self) -> Result<Option<FrameKind>, ReadError> {
        // let prev_pos = self.pos;
        loop {
            match parse_frame(self.buf, self.pos) {
                Ok(Some((frame, next))) => {
                    self.pos = next;
                    // println!("frame: i={prev_pos}, next={next}, v={frame:?}");
                    return Ok(Some(frame));
                }
                Ok(None) => {
                    // Fill buffer and retry
                    let n = self.reader.read_buf(&mut self.buf).await?;
                    if n == 0 {
                        return Ok(None);
                    }
                }
                Err(ParseError::IncompleteBuffer) => {
                    // Incomplete, ask more bytes
                    let n = self.reader.read_buf(&mut self.buf).await?;
                    if n == 0 {
                        return Err(ParseError::IncompleteBuffer.into());
                    }
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
}
