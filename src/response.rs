use bytes::{BufMut, Bytes, BytesMut};

pub enum Response<'a> {
    Ok,
    Err { code: Option<&'a str>, err: &'a str },
    BulkString(Bytes),
    NullBulkString,
    Integer(u64),
    Array(Vec<Bytes>),
}

pub struct BufferBuilder {
    buf: BytesMut,
}

/// Simple buffered response builder.
///
/// Not recommended for huge payloads.
impl BufferBuilder {
    pub fn new(buf: BytesMut) -> Self {
        Self { buf }
    }

    pub fn default() -> Self {
        Self::new(BytesMut::with_capacity(4096))
    }

    pub fn err(&mut self, code: Option<&str>, msg: &str) {
        self.buf.put_u8(b'-');
        self.buf.extend_from_slice(code.unwrap_or("ERR").as_bytes());
        self.buf.put_u8(b' ');
        self.buf.extend_from_slice(msg.as_bytes());
        self.buf.extend_from_slice(b"\r\n");
    }

    pub fn ok(&mut self) {
        self.buf.extend_from_slice(b"+OK\r\n");
    }

    pub fn pong(&mut self) {
        self.buf.extend_from_slice(b"+PONG\r\n");
    }

    pub fn null_bulk_str(&mut self) {
        self.buf.extend_from_slice(b"$-1\r\n");
    }

    pub fn integer(&mut self, n: usize) {
        self.buf.put_u8(b':');
        self.buf.extend_from_slice(n.to_string().as_bytes());
        self.eol();
    }

    pub fn array(&mut self, len: usize) {
        self.buf.put_u8(b'*');
        self.buf.extend_from_slice(len.to_string().as_bytes());
        self.eol();
    }

    fn eol(&mut self) {
        self.buf.extend_from_slice(b"\r\n");
    }

    pub fn str_simple(&mut self, msg: &[u8]) {
        self.buf.put_u8(b'+');
        self.buf.extend_from_slice(msg);
        self.eol();
    }

    pub fn str_bulk(&mut self, msg: &[u8]) {
        self.buf.put_u8(b'$');
        self.buf.extend_from_slice(msg.len().to_string().as_bytes());
        self.eol();
        self.buf.extend_from_slice(msg);
        self.eol();
    }

    pub fn build(&mut self) -> Bytes {
        self.buf.split().freeze()
    }
}
