use std::num::TryFromIntError;

use crate::parser::BufRef;
use bytes::Bytes;
use thiserror::Error;

#[derive(Debug)]
pub enum ValueRef {
    Array { len: u64 },
    String(BufRef),
}

#[derive(Debug)]
pub enum Value {
    String(Bytes),
    Array(Vec<Value>),
}

#[derive(Debug, Error)]
pub enum ValueTypeError {
    #[error("value is not a scalar value")]
    NotAScalar,

    #[error("value is not a string")]
    NotAString,

    #[error("invalid utf8 value")]
    Utf8Error(#[from] std::str::Utf8Error),

    #[error("not an integer")]
    ParseIntError(#[from] std::num::ParseIntError),
}

impl<'a> TryFrom<&'a Value> for &'a Bytes {
    type Error = ValueTypeError;

    fn try_from(value: &'a Value) -> Result<Self, Self::Error> {
        if let Value::String(val) = value {
            Ok(val)
        } else {
            Err(ValueTypeError::NotAScalar)
        }
    }
}

impl TryFrom<&Value> for i32 {
    type Error = ValueTypeError;

    fn try_from(value: &Value) -> Result<Self, Self::Error> {
        if let Value::String(val) = value {
            let s = std::str::from_utf8(val.as_ref())?;
            Ok(s.parse::<i32>()?)
        } else {
            Err(ValueTypeError::NotAScalar)
        }
    }
}

impl TryFrom<&Value> for u64 {
    type Error = ValueTypeError;

    fn try_from(value: &Value) -> Result<Self, Self::Error> {
        if let Value::String(val) = value {
            let s = std::str::from_utf8(val.as_ref())?;
            Ok(s.parse::<u64>()?)
        } else {
            Err(ValueTypeError::NotAScalar)
        }
    }
}

#[derive(Debug, Error)]
pub enum AssembleError {
    #[error("missing array item")]
    MissingArrayItem { array_len: usize, index: usize },

    #[error("Length too large")]
    LengthTooLarge(#[from] TryFromIntError),
}

pub fn assemble(buf: Bytes, refs: &[ValueRef]) -> Result<Option<Value>, AssembleError> {
    let Some(val) = refs.first() else {
        return Ok(None);
    };

    let refs = &refs[1..];
    Ok(Some(assemble_value(&buf, val, refs)?.value))
}

struct AssembleChunk {
    value: Value,
    read_items: usize,
}

fn assemble_value(
    buf: &Bytes,
    head: &ValueRef,
    tail: &[ValueRef],
) -> Result<AssembleChunk, AssembleError> {
    match head {
        ValueRef::Array { len } => assemble_array(buf, tail, *len),
        ValueRef::String(r) => Ok(assemble_string(buf, r)),
    }
}

fn assemble_array(
    buf: &Bytes,
    tail: &[ValueRef],
    len: u64,
) -> Result<AssembleChunk, AssembleError> {
    let len = len.try_into()?;
    let mut collected = Vec::with_capacity(len);
    let mut refs = tail;
    let mut drained_count: usize = 0;
    for i in 0..len {
        let r = refs.first().ok_or(AssembleError::MissingArrayItem {
            array_len: len,
            index: i,
        })?;

        let chunk = assemble_value(buf, r, refs)?;
        drained_count += chunk.read_items;
        refs = &refs[chunk.read_items..];
        collected.push(chunk.value);
    }

    Ok(AssembleChunk {
        value: Value::Array(collected),
        read_items: drained_count + 1, // include array itself
    })
}
fn assemble_string(buf: &Bytes, r: &BufRef) -> AssembleChunk {
    AssembleChunk {
        value: Value::String(buf.slice(r.offset()..r.end())),
        read_items: 1,
    }
}
