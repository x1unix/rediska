use crate::assembler::Value;
use bytes::Bytes;

use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[derive(Debug, Error)]
pub enum RequestError {
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
}

pub enum Request {
    Ping,
    Echo { msg: Bytes },
    // TODO: add another commands
}

impl Request {
    fn new_ping(args: &[Value]) -> Result<Self, RequestError> {
        if args.is_empty() {
            Ok(Self::Ping)
        } else {
            Err(RequestError::InvalidArgsCount {
                want: 0,
                got: args.len(),
                cmd: "PING",
            })
        }
    }

    fn new_echo(args: &[Value]) -> Result<Self, RequestError> {
        if args.len() != 1 {
            return Err(RequestError::InvalidArgsCount {
                want: 1,
                got: args.len(),
                cmd: "ECHO",
            });
        }

        // TODO: support multiple strings?
        let v = &args[0];
        match v {
            Value::String(b) => Ok(Self::Echo { msg: b.to_owned() }),
            _ => Err(RequestError::InvalidArgumentType { pos: 0 }),
        }
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
            _ => Err(RequestError::UnknownCommand(cmd.to_owned())),
        }
    }
}
