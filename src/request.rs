use std::time::{Duration, SystemTime, SystemTimeError, UNIX_EPOCH};

use crate::{
    assembler::{Value, ValueTypeError},
    parser::ParseError,
    reader::ReadError,
};
use bytes::Bytes;

use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[derive(Debug, Error)]
pub enum RequestError {
    #[error("cannot read request")]
    ReadError(#[from] ReadError),

    #[error("unknown command")]
    UnknownCommand(Bytes),

    #[error("empty command")]
    EmptyCommand,

    #[error("invalid payload")]
    InvalidPayload(Value),

    #[error("invalid command key type")]
    InvalidCommandKeyType,

    #[error("invalid argument")]
    InvalidArgumentType { pos: usize, err: ValueTypeError },

    #[error("invalid args count")]
    InvalidArgsCount {
        want: usize,
        got: usize,
        cmd: &'static str,
    },

    #[error("value is out of range, must be positive")]
    OutOfRange,

    #[error("missing key")]
    MissingKey { cmd: &'static str },

    #[error("missing option value")]
    MissingOptValue { cmd: &'static str, pos: usize },
}

struct ArgReader<'a> {
    cmd: &'static str,
    args: &'a [Value],
    offset: usize,
}

impl<'a> ArgReader<'a> {
    fn new(cmd: &'static str, args: &'a [Value]) -> Self {
        Self {
            cmd,
            args,
            offset: 0,
        }
    }

    fn is_empty(&self) -> bool {
        self.args.is_empty()
    }

    fn len(&self) -> usize {
        self.args.len()
    }

    fn advance(&mut self, n: usize) {
        self.offset += n;
        self.args = &self.args[n..];
    }

    fn str(&mut self) -> Result<Bytes, RequestError> {
        match self.args.first() {
            Some(Value::String(b)) => {
                self.advance(1);
                Ok(b.to_owned())
            }
            Some(_) => Err(RequestError::InvalidArgumentType {
                pos: self.offset,
                err: ValueTypeError::NotAString,
            }),
            None => Err(RequestError::InvalidArgsCount {
                want: self.offset + 1,
                got: self.offset,
                cmd: self.cmd,
            }),
        }
    }

    fn next<T>(&mut self) -> Result<T, RequestError>
    where
        T: TryFrom<&'a Value, Error = ValueTypeError>,
    {
        let x = self
            .args
            .first()
            .map(|v| T::try_from(v))
            .transpose()
            .map_err(|e| RequestError::InvalidArgumentType {
                pos: self.offset,
                err: e,
            })?;

        if let Some(x) = x {
            self.advance(1);
            Ok(x)
        } else {
            Err(RequestError::InvalidArgsCount {
                want: self.offset + 1,
                got: self.offset,
                cmd: self.cmd,
            })
        }
    }

    fn kv(&mut self) -> Result<(Bytes, Bytes), RequestError> {
        if self.args.len() < 2 {
            return Err(RequestError::InvalidArgsCount {
                want: 2,
                got: self.args.len(),
                cmd: self.cmd,
            });
        }

        let parts = &self.args[0..2]
            .iter()
            .enumerate()
            .map(|(i, v)| match v {
                Value::String(b) => {
                    if i == 0 && b.is_empty() {
                        Err(RequestError::MissingKey { cmd: self.cmd })
                    } else {
                        Ok(b.to_owned())
                    }
                }
                _ => Err(RequestError::InvalidArgumentType {
                    pos: i,
                    err: ValueTypeError::NotAString,
                }),
            })
            .collect::<Result<Vec<Bytes>, RequestError>>()?;

        let k = parts[0].clone();
        let v = parts[1].clone();

        self.advance(2);
        Ok((k, v))
    }

    fn get_opt(&mut self, key: &[u8]) -> Result<Option<&Value>, RequestError> {
        // first some && first is string && first eq key
        let ok = match self.args.first() {
            Some(Value::String(k)) => k.as_ref().eq_ignore_ascii_case(key),
            _ => false,
        };

        if !ok {
            return Ok(None);
        }

        if let Some(v) = self.args.get(1) {
            self.advance(2);
            Ok(Some(v))
        } else {
            Err(RequestError::MissingOptValue {
                cmd: self.cmd,
                pos: self.offset,
            })
        }
    }

    fn get_opt_u64(&mut self, key: &[u8]) -> Result<Option<u64>, RequestError> {
        let offset = self.offset;
        self.get_opt(key)?
            .map(|v| {
                u64::try_from(v).map_err(|e| RequestError::InvalidArgumentType {
                    pos: offset + 2,
                    err: e,
                })
            })
            .transpose()
    }

    fn assert_empty(&self) -> Result<(), RequestError> {
        if self.args.is_empty() {
            Ok(())
        } else {
            Err(RequestError::InvalidArgsCount {
                want: self.offset,
                got: self.offset + self.args.len(),
                cmd: self.cmd,
            })
        }
    }

    fn collect_strs(&self) -> Result<Vec<Bytes>, RequestError> {
        if self.args.is_empty() {
            return Err(RequestError::InvalidArgsCount {
                want: self.offset + 1,
                got: self.offset,
                cmd: self.cmd,
            });
        }

        let offset = self.offset;
        self.args
            .iter()
            .enumerate()
            .map(|(i, e)| match e {
                // Bytes are referencing to request payload region.
                // Copy values to avoid leaking memory.
                Value::String(b) => Ok(Bytes::copy_from_slice(b)),
                _ => Err(RequestError::InvalidArgumentType {
                    pos: offset + i,
                    err: ValueTypeError::NotAScalar,
                }),
            })
            .collect()
    }
}

pub enum Ttl {
    Duration(Duration),
    Timestamp(Duration),
}

impl TryInto<Duration> for Ttl {
    type Error = SystemTimeError;

    fn try_into(self) -> Result<Duration, Self::Error> {
        self.as_unix()
    }
}

impl Ttl {
    /// Returns Unix timestamp duration from TTL value based on current system time.
    pub fn as_unix(&self) -> Result<Duration, SystemTimeError> {
        match self {
            Ttl::Duration(dur) => SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|now| now + dur.to_owned()),
            Ttl::Timestamp(ts) => Ok(ts.to_owned()),
        }
    }
}

pub enum Request {
    Ping,
    Echo {
        msg: Bytes,
    },
    Get {
        key: Bytes,
    },
    Set {
        key: Bytes,
        val: Bytes,
        ttl: Option<Ttl>,
    },
    Rpush {
        key: Bytes,
        values: Vec<Bytes>,
    },
    Lpush {
        key: Bytes,
        values: Vec<Bytes>,
    },
    Lrange {
        key: Bytes,
        start: i32,
        end: i32,
    },
    Llen {
        key: Bytes,
    },
    Lpop {
        key: Bytes,
        n: usize,
    },
    // TODO: add another commands
}

impl Request {
    fn new_ping(args: &[Value]) -> Result<Self, RequestError> {
        let r = ArgReader::new("PING", args);
        r.assert_empty()?;
        Ok(Self::Ping)
    }

    fn new_echo(args: &[Value]) -> Result<Self, RequestError> {
        let mut r = ArgReader::new("ECHO", args);
        let msg = r.str()?;
        r.assert_empty()?;

        // TODO: support multiple strings?
        Ok(Self::Echo { msg })
    }

    fn new_set(args: &[Value]) -> Result<Self, RequestError> {
        let mut r = ArgReader::new("SET", args);

        let (key, val) = r.kv()?;

        // TODO: this logic is brittle and relies on args ordering.
        let ttl = if let Some(v) = r.get_opt_u64(b"EX")? {
            Some(Ttl::Duration(Duration::from_secs(v)))
        } else if let Some(v) = r.get_opt_u64(b"PX")? {
            Some(Ttl::Duration(Duration::from_millis(v)))
        } else if let Some(v) = r.get_opt_u64(b"EXAT")? {
            Some(Ttl::Timestamp(Duration::from_secs(v)))
        } else {
            // Clippy warns about manual_map unless I do this:
            r.get_opt_u64(b"PXAT")?
                .map(|v| Ttl::Timestamp(Duration::from_millis(v)))
        };

        // TODO: support NX, XX, IFEQ, IFDEQ, etc options.
        r.assert_empty()?;
        Ok(Self::Set { key, val, ttl })
    }

    fn new_get(args: &[Value]) -> Result<Self, RequestError> {
        let mut r = ArgReader::new("GET", args);
        let key = r.str()?;
        r.assert_empty()?;

        Ok(Self::Get { key })
    }

    fn new_llen(args: &[Value]) -> Result<Self, RequestError> {
        let mut r = ArgReader::new("LLEN", args);
        let key = r.str()?;
        r.assert_empty()?;

        Ok(Self::Llen { key })
    }

    fn new_lpush(args: &[Value]) -> Result<Self, RequestError> {
        let mut r = ArgReader::new("LPUSH", args);
        let key = r.str()?;
        let values = r.collect_strs()?;
        Ok(Self::Lpush { key, values })
    }

    fn new_rpush(args: &[Value]) -> Result<Self, RequestError> {
        let mut r = ArgReader::new("RPUSH", args);
        let key = r.str()?;
        let values = r.collect_strs()?;
        Ok(Self::Rpush { key, values })
    }

    fn new_lrange(args: &[Value]) -> Result<Self, RequestError> {
        let mut r = ArgReader::new("LRANGE", args);
        let key = r.str()?;
        let start = r.next::<i32>()?;
        let end = r.next::<i32>()?;

        Ok(Self::Lrange { key, start, end })
    }

    fn new_lpop(args: &[Value]) -> Result<Self, RequestError> {
        let mut r = ArgReader::new("LPOP", args);
        let key = r.str()?;
        r.next::<usize>().and_then(|n| {
            if n > 0 {
                Ok(Self::Lpop { key, n })
            } else {
                Err(RequestError::OutOfRange)
            }
        })
    }
}

impl TryFrom<Value> for Request {
    type Error = RequestError;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        let Value::Array(arr) = value else {
            return Err(RequestError::InvalidPayload(value));
        };

        // TODO: support pipelines, batches, etc.
        let cmd = arr
            .first()
            .ok_or(RequestError::EmptyCommand)
            .and_then(|v| match v {
                Value::String(b) => {
                    if b.is_empty() {
                        Err(RequestError::EmptyCommand)
                    } else {
                        Ok(b)
                    }
                }
                _ => Err(RequestError::InvalidCommandKeyType),
            })?;

        let args = &arr[1..];

        // Redis commands are case-insensitive
        match cmd.as_ref() {
            cmd if cmd.eq_ignore_ascii_case(b"PING") => Self::new_ping(args),
            cmd if cmd.eq_ignore_ascii_case(b"ECHO") => Self::new_echo(args),
            cmd if cmd.eq_ignore_ascii_case(b"GET") => Self::new_get(args),
            cmd if cmd.eq_ignore_ascii_case(b"SET") => Self::new_set(args),
            cmd if cmd.eq_ignore_ascii_case(b"LLEN") => Self::new_llen(args),
            cmd if cmd.eq_ignore_ascii_case(b"RPUSH") => Self::new_rpush(args),
            cmd if cmd.eq_ignore_ascii_case(b"LPUSH") => Self::new_lpush(args),
            cmd if cmd.eq_ignore_ascii_case(b"LRANGE") => Self::new_lrange(args),
            cmd if cmd.eq_ignore_ascii_case(b"LPOP") => Self::new_lpop(args),
            _ => Err(RequestError::UnknownCommand(cmd.to_owned())),
        }
    }
}
