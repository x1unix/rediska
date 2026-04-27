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
pub enum AssembleError {
    #[error("missing array item")]
    MissingArrayItem { array_len: usize, index: usize },

    #[error("Length too large")]
    LengthTooLarge(#[from] TryFromIntError),
}

pub fn assemble(buf: Bytes, refs: &[ValueRef]) -> Result<Option<Value>, AssembleError> {
    let Some(val) = refs.get(0) else {
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
    return match head {
        ValueRef::Array { len } => assemble_array(buf, tail, *len),
        ValueRef::String(r) => Ok(assemble_string(buf, r)),
    };
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
        let r = refs.get(0).ok_or(AssembleError::MissingArrayItem {
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
