use std::net::TcpStream;

use anyhow::{Context, Error, Result, anyhow};

pub fn read_stream<T>(s: &mut T) -> Result<()>
where
    T: std::io::Read + std::io::Write,
{
    // TODO: read all
    let mut buff: [u8; 8] = [0; 8];
    let n = s.read(&mut buff[..])?;
    let _req = str::from_utf8(&buff[..n]).context("payload is not a text")?;
    // let response = match req {
    //     "PING" => "+PONG\r\n",
    //     _ => return Err(anyhow!("invalid cmd: {}", req)),
    // };
    let response = "+PONG\r\n";

    s.write_all(response.as_bytes())
        .context("can't write response")
}
