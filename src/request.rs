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
}

pub enum TTL {
    Duration(Duration),
    Timestamp(Duration),
}

impl TryInto<Duration> for TTL {
    type Error = SystemTimeError;

    fn try_into(self) -> Result<Duration, Self::Error> {
        self.as_unix()
    }
}

impl TTL {
    /// Returns Unix timestamp duration from TTL value based on current system time.
    pub fn as_unix(&self) -> Result<Duration, SystemTimeError> {
        match self {
            TTL::Duration(dur) => SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|now| now + dur.to_owned()),
            TTL::Timestamp(ts) => Ok(ts.to_owned()),
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
        ttl: Option<TTL>,
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
            Some(TTL::Duration(Duration::from_secs(v)))
        } else if let Some(v) = r.get_opt_u64(b"PX")? {
            Some(TTL::Duration(Duration::from_millis(v)))
        } else if let Some(v) = r.get_opt_u64(b"EXAT")? {
            Some(TTL::Timestamp(Duration::from_secs(v)))
        } else {
            // Clippy warns about manual_map unless I do this:
            r.get_opt_u64(b"PXAT")?
                .map(|v| TTL::Timestamp(Duration::from_millis(v)))
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
}

impl TryFrom<Value> for Request {
    type Error = RequestError;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        let Value::Array(arr) = value else {
            return Err(RequestError::InvalidPayload(value));
        };

        // TODO: support pipelines, batches, etc.
        let cmd = arr
            .get(0)
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
            _ => Err(RequestError::UnknownCommand(cmd.to_owned())),
        }
    }
}
