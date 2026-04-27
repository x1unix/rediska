use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[derive(Debug)]
pub enum FrameKind {
    Array { len: u64, width: usize },
    BulkString { len: u64, width: usize },
    NullBulkString,
    Delimiter,
}

impl FrameKind {
    /// Returns frame size in bytes
    pub fn byte_len(&self) -> usize {
        match self {
            Self::Delimiter => 2,
            Self::NullBulkString => 3,
            Self::BulkString { width, .. } => *width,
            Self::Array { width, .. } => *width,
        }
    }
}

#[derive(Debug)]
pub struct BufRef(usize, usize);

impl BufRef {
    pub fn new(offset: usize, len: usize) -> BufRef {
        BufRef(offset, len)
    }

    pub fn offset(&self) -> usize {
        self.0
    }

    pub fn is_empty(&self) -> bool {
        self.1 == 0
    }

    pub fn len(&self) -> usize {
        self.1
    }

    pub fn end(&self) -> usize {
        self.0 + self.1
    }

    pub fn with_offset(&self, offset: usize) -> Self {
        BufRef(offset + self.0, self.1)
    }
}

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("incomplete buffer")]
    IncompleteBuffer,

    #[error("bad frame")]
    BadFrame(BufRef),

    #[error("invalid length value")]
    BadLength((BufRef, i64)),

    #[error("cannot parse length")]
    NanLength((BufRef, u8)),

    #[error("unexpected frame")]
    UnexpectedFrame { offset: usize, frame: FrameKind },

    #[error("unknown frame")]
    UnknownFrame { offset: usize, val: u8 },
}

impl ParseError {
    /// with_offset adds base offset to error positions.
    pub fn with_offset(self, addr: usize) -> ParseError {
        match self {
            Self::BadFrame(r) => Self::BadFrame(r.with_offset(addr)),
            Self::BadLength((r, l)) => Self::BadLength((r.with_offset(addr), l)),
            Self::NanLength((r, l)) => Self::NanLength((r.with_offset(addr), l)),
            Self::UnexpectedFrame { offset, frame } => Self::UnexpectedFrame {
                offset: offset + addr,
                frame,
            },
            Self::UnknownFrame { offset, val } => Self::UnknownFrame {
                offset: offset + addr,
                val,
            },
            _ => self,
        }
    }
}

pub enum ValueRef {
    Array { len: u64 },
    String(BufRef),
    // TODO: add remaining types
}

pub fn parse_frame(src: &[u8], offset: usize) -> Result<Option<(FrameKind, usize)>, ParseError> {
    let ch = match src.get(offset) {
        Some(ch) => ch,
        None => return Ok(None),
    };

    // TODO: support all frame types
    // https://redis.io/docs/latest/develop/reference/protocol-spec/#resp-protocol-description
    match *ch {
        b'\r' => match src.get(offset + 1) {
            None => Err(ParseError::IncompleteBuffer),
            Some(b'\n') => Ok(Some((FrameKind::Delimiter, 2))),
            _ => Err(ParseError::BadFrame(BufRef(offset, 1))),
        },
        b'*' => {
            // TODO: maybe support null arrays (*-1)?
            let (len, next) = read_uint(src, offset + 1)?;
            let width = next - offset; // Size of "*<digits...>" segment w/o CRLF
            Ok(Some((FrameKind::Array { len, width }, next)))
        }
        b'$' => {
            let (len, next) = read_int(src, offset + 1, true)?;
            match len {
                -1 => Ok(Some((FrameKind::NullBulkString, next))),
                x if x >= 0 => Ok(Some((
                    FrameKind::BulkString {
                        len: x as u64,
                        width: next - offset, // Size of "$<digits...>" segment w/o CRLF
                    },
                    next,
                ))),
                _ => Err(ParseError::BadLength((BufRef(offset, next), len))),
            }
        }
        _ => Err(ParseError::UnknownFrame { offset, val: *ch }),
    }
}

pub fn read_uint(src: &[u8], offset: usize) -> Result<(u64, usize), ParseError> {
    let (val, next) = read_int(src, offset, false)?;
    u64::try_from(val)
        .map(|v| (v, next))
        .map_err(|_| ParseError::BadLength((BufRef(offset, next), val)))
}

/// Reads a given buffer from offset and reads an integer value till carriage return character (\n).
/// Returns read value and offset after a numeric string.
pub fn read_int(src: &[u8], offset: usize, signed: bool) -> Result<(i64, usize), ParseError> {
    let is_neg = match src.get(offset) {
        None => return Err(ParseError::IncompleteBuffer),
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
                return if is_empty || (!signed && acc < 0) {
                    Err(ParseError::BadLength((BufRef(offset, i - offset), acc)))
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
                    .ok_or(ParseError::BadLength((BufRef(offset, i - offset), acc)))?;
                i += 1;
            }
            Some(c) => return Err(ParseError::NanLength((BufRef(offset, i - offset), *c))),
            None => return Err(ParseError::IncompleteBuffer),
        }
    }

    // need more data to read till delimiter
    Err(ParseError::IncompleteBuffer)
}

/// Validates the bulk string frame and returns its ref on success.
pub fn check_word(src: &[u8], offset: usize, len: usize) -> Result<BufRef, ParseError> {
    // NOTE: redis-cli split strings into separate frames only by "\n". "foo\rbar" is single frame.
    let cr = offset + len;
    let lf = cr + 1;
    match (src.get(cr), src.get(lf)) {
        (Some(b'\r'), Some(b'\n')) => Ok(BufRef::new(offset, len)),
        (_, None) => Err(ParseError::IncompleteBuffer),
        _ => Err(ParseError::BadFrame(BufRef::new(offset, len))),
    }
}
