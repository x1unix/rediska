#![allow(unused_imports)]
use anyhow::Result;
// use std::net::TcpListener;
use tokio::net::{TcpListener, TcpStream};

pub mod proto;

const ADDR: &str = "127.0.0.1:6379";

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let listener = TcpListener::bind(ADDR).await?;
    println!("Listening on {ADDR}");

    loop {
        let (sock, addr) = listener.accept().await?;
        println!("Conn: {addr}");
        if let Err(e) = handle_incoming(sock).await {
            println!("Error: {e}");
        }
    }
}

async fn handle_incoming(mut s: TcpStream) -> Result<()> {
    loop {
        if proto::read_stream(&mut s).await?.is_none() {
            break;
        }
    }
    Ok(())
}
