//! Tokio-based echo server for benchmark comparison.
//!
//! Usage: cargo run --example tokio_echo
//!
//! This creates a simple TCP echo server on port 12345 using tokio.
//! Uses the same number of worker threads as exec-rs examples for fair comparison.

use std::error::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const NUM_THREADS: usize = 4;

fn main() -> Result<(), Box<dyn Error>> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(NUM_THREADS)
        .enable_all()
        .build()?;

    runtime.block_on(async_main())
}

async fn async_main() -> Result<(), Box<dyn Error>> {
    let addr = "127.0.0.1:12345";
    let listener = TcpListener::bind(addr).await?;
    println!("Tokio echo server listening on {} ({} worker threads)", addr, NUM_THREADS);

    loop {
        let (mut socket, _addr) = listener.accept().await?;

        tokio::spawn(async move {
            let mut buf = [0u8; 4096];

            loop {
                let n = match socket.read(&mut buf).await {
                    Ok(0) => return, // Connection closed
                    Ok(n) => n,
                    Err(_) => return,
                };

                if socket.write_all(&buf[..n]).await.is_err() {
                    return;
                }
            }
        });
    }
}
