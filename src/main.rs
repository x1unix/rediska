#![allow(unused_imports)]
use anyhow::Result;
// use std::net::TcpListener;
use tokio::net::{TcpListener, TcpStream};

pub mod assembler;
pub mod parser;
pub mod reader;
pub mod server;

const ADDR: &str = "127.0.0.1:6379";

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let listener = TcpListener::bind(ADDR).await?;
    println!("Listening on {ADDR}");

    loop {
        let (sock, addr) = listener.accept().await?;
        tokio::spawn(async move {
            if let Err(e) = handle_incoming(addr, sock).await {
                println!("Error: {e}");
            }
        });
    }
}

async fn handle_incoming(addr: std::net::SocketAddr, mut s: TcpStream) -> Result<()> {
    println!("Conn: {addr}");
    loop {
        if proto::read_stream(&mut s).await?.is_none() {
            break;
        }
    }
    Ok(())
}
