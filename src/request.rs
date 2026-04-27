use crate::{assembler::Value, parser::ParseError, reader::ReadError};
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
    InvalidArgumentType { pos: usize },

    #[error("invalid args count")]
    InvalidArgsCount {
        want: usize,
        got: usize,
        cmd: &'static str,
    },

    #[error("missing key")]
    MissingKey { cmd: &'static str },
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
        match self.args.get(0) {
            Some(Value::String(b)) => {
                self.advance(1);
                Ok(b.to_owned())
            }
            Some(_) => Err(RequestError::InvalidArgumentType { pos: self.offset }),
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
                _ => Err(RequestError::InvalidArgumentType { pos: i }),
            })
            .collect::<Result<Vec<Bytes>, RequestError>>()?;

        let k = parts[0].clone();
        let v = parts[1].clone();

        self.advance(2);
        Ok((k, v))
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

pub enum Request {
    Ping,
    Echo { msg: Bytes },
    Get { key: Bytes },
    Set { key: Bytes, val: Bytes, ttl: u64 },
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

        // TODO: support TTL
        let (k, v) = r.kv()?;
        r.assert_empty()?;
        Ok(Self::Set {
            key: k,
            val: v,
            ttl: 0,
        })
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
