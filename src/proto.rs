use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use anyhow::{Context, Error, Result, anyhow};

pub enum ValueKind {}

pub async fn read_stream<T>(s: &mut T) -> Result<Option<()>>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    // TODO: read all
    let mut buff: [u8; 16] = [0; 16];
    let n = s.read(&mut buff[..]).await?;
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
        .await
        .context("can't write response")?;

    Ok(Some(()))
}
