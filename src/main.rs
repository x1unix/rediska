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
    let listener = TcpListener::bind(ADDR).await?;
    println!("Listening on {ADDR}");

    loop {
        let (sock, addr) = listener.accept().await?;
        tokio::spawn(async move {
            if let Err(e) = server::handle_conn(addr, sock).await {
                println!("Error: {e}");
            }
        });
    }
}
