#![allow(unused_imports)]
use anyhow::Result;
use tokio::net::{TcpListener, TcpStream};

pub mod assembler;
pub mod parser;
pub mod reader;
pub mod request;
pub mod server;

const ADDR: &str = "127.0.0.1:6379";

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    if let Err(err) = server::listen(ADDR).await {
        println!("Error: {err}");
    };

    Ok(())
}

// pub async fn read_stream<T>(s: &mut T) -> anyhow::Result<Option<()>>
// where
//     T: AsyncRead + AsyncWrite + Unpin,
// {
//     // TODO: read all
//     // let mut buff: [u8; 4] = [0; 4];
//     let mut buff: [u8; 32] = [0; 32];
//     let n = s.read(&mut buff[..]).await?;
//     if n == 0 {
//         return Ok(None);
//     }
//
//     let req = str::from_utf8(&buff[..n]).context("payload is not a text")?;
//     println!("Req: {:?}", req);
//     // let response = match req {
//     //     "PING" => "+PONG\r\n",
//     //     _ => return Err(anyhow!("invalid cmd: {}", req)),
//     // };
//     let response = "+PONG\r\n";
//
//     s.write_all(response.as_bytes())
//         .await
//         .context("can't write response")?;
//
//     // Ok(Some(()))
//     Ok(None)
// }
//
