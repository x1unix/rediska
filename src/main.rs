#![allow(unused_imports)]
use anyhow::Result;
use std::net::TcpListener;

pub mod proto;

fn main() {
    let listener = TcpListener::bind("127.0.0.1:6379").unwrap();

    for stream in listener.incoming() {
        if let Err(err) = handle_incoming(stream) {
            println!("error: {}", err);
        }
    }
}

fn handle_incoming(r: Result<std::net::TcpStream, std::io::Error>) -> Result<()> {
    let mut s = r?;
    proto::read_stream(&mut s)?;
    Ok(())
}
