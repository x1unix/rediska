use std::net::TcpStream;

use anyhow::{Context, Error, Result, anyhow};

pub enum ValueKind {}

pub fn read_stream<T>(s: &mut T) -> Result<Option<()>>
where
    T: std::io::Read + std::io::Write,
{
    // TODO: read all
    let mut buff: [u8; 16] = [0; 16];
    let n = s.read(&mut buff[..])?;
    if n == 0 {
        return Ok(None);
    }

    let _req = str::from_utf8(&buff[..n]).context("payload is not a text")?;
    // let response = match req {
    //     "PING" => "+PONG\r\n",
    //     _ => return Err(anyhow!("invalid cmd: {}", req)),
    // };
    let response = "+PONG\r\n";

    s.write_all(response.as_bytes())
        .context("can't write response")?;

    Some(())
}
