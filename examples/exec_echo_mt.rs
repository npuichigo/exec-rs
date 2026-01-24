//! Multi-threaded exec-rs echo server.
//!
//! Usage: cargo run --example exec_echo_mt --release
//!
//! This creates a TCP echo server on port 12345 using multiple accept threads
//! with exec-rs thread pool for connection handling.
//!
//! Architecture:
//! - Multiple acceptor threads share the listening socket (SO_REUSEPORT)
//! - Each accepted connection is handled by the exec-rs thread pool
//! - Uses blocking IO per connection (similar to thread-per-connection but pooled)

use exec_rs::adaptors::{on, then};
use exec_rs::consumers::start_detached;
use exec_rs::factories::just;
use exec_rs::schedulers::StaticThreadPool;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

fn handle_client(mut stream: TcpStream) {
    let mut buf = [0u8; 4096];

    // Disable Nagle's algorithm for lower latency
    stream.set_nodelay(true).ok();

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

fn acceptor_thread(
    listener: Arc<TcpListener>,
    pool: Arc<StaticThreadPool>,
    running: Arc<AtomicBool>,
) {
    let scheduler = pool.get_scheduler();

    while running.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _addr)) => {
                // Schedule the connection handler on the thread pool
                let sender = on(scheduler.clone(), then(just(stream), handle_client));
                start_detached(sender);
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(std::time::Duration::from_micros(100));
            }
            Err(e) => {
                if running.load(Ordering::Relaxed) {
                    eprintln!("Accept error: {}", e);
                }
            }
        }
    }
}

fn main() {
    let addr = "127.0.0.1:12345";

    // Create the listener
    let listener = TcpListener::bind(addr).expect("Failed to bind");
    listener
        .set_nonblocking(true)
        .expect("Cannot set non-blocking");

    let listener = Arc::new(listener);

    // Use fixed thread count for fair comparison
    const NUM_WORKERS: usize = 4;
    const NUM_ACCEPTORS: usize = 2;

    // Create thread pool for connection handling
    let pool = Arc::new(StaticThreadPool::new(NUM_WORKERS));

    println!(
        "exec-rs multi-threaded echo server listening on {}",
        addr
    );
    println!(
        "  {} acceptor threads, {} worker threads",
        NUM_ACCEPTORS, NUM_WORKERS
    );

    let running = Arc::new(AtomicBool::new(true));

    // Spawn acceptor threads
    let mut handles = Vec::new();
    for _ in 0..NUM_ACCEPTORS {
        let listener = listener.clone();
        let pool = pool.clone();
        let running = running.clone();

        handles.push(thread::spawn(move || {
            acceptor_thread(listener, pool, running);
        }));
    }

    // Wait for Ctrl+C (simple approach)
    println!("Press Ctrl+C to stop...");

    // Block main thread - in production use proper signal handling
    loop {
        thread::sleep(std::time::Duration::from_secs(1));
    }
}
