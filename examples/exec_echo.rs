//! exec-rs based echo server using thread pool with blocking IO.
//!
//! Usage: cargo run --example exec_echo --release
//!
//! This creates a TCP echo server on port 12345 using exec-rs thread pool
//! for concurrent connection handling with standard blocking IO.

use exec_rs::adaptors::then;
use exec_rs::consumers::start_detached;
use exec_rs::factories::just;
use exec_rs::schedulers::StaticThreadPool;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

fn handle_client(mut stream: TcpStream) {
    let mut buf = [0u8; 4096];

    loop {
        let n = match stream.read(&mut buf) {
            Ok(0) => return, // Connection closed
            Ok(n) => n,
            Err(_) => return,
        };

        if stream.write_all(&buf[..n]).is_err() {
            return;
        }
    }
}

fn main() {
    let addr = "127.0.0.1:12345";
    let listener = TcpListener::bind(addr).expect("Failed to bind");
    println!("exec-rs echo server listening on {}", addr);

    // Create a thread pool with 4 threads for fair comparison
    const NUM_THREADS: usize = 4;
    let pool = StaticThreadPool::new(NUM_THREADS);
    println!("  {} worker threads", NUM_THREADS);
    let scheduler = pool.get_scheduler();

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    // Handle Ctrl+C
    ctrlc_handler(r);

    // Set non-blocking to allow checking running flag periodically
    listener
        .set_nonblocking(true)
        .expect("Cannot set non-blocking");

    while running.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _addr)) => {
                // Set blocking for the client stream
                stream.set_nonblocking(false).ok();

                // Schedule the connection handler on the thread pool
                let sender = then(just(stream), |stream| {
                    handle_client(stream);
                });

                // Use on() to run on the thread pool
                let scheduled = exec_rs::adaptors::on(scheduler.clone(), sender);
                start_detached(scheduled);
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No connection ready, sleep briefly
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Err(e) => {
                eprintln!("Accept error: {}", e);
            }
        }
    }

    println!("Shutting down...");
}

fn ctrlc_handler(_running: Arc<AtomicBool>) {
    // For simplicity, just let the server run until killed
    // In a real application, use proper signal handling (e.g., signal-hook crate)
}
