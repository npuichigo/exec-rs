//! exec-rs based echo server using mio for event-driven IO.
//!
//! Usage: cargo run --example exec_echo_mio --release
//!
//! This creates a TCP echo server on port 12345 using mio for the event loop
//! and exec-rs for task coordination.

use mio::net::{TcpListener, TcpStream};
use mio::{Events, Interest, Poll, Token};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const SERVER: Token = Token(0);
const BUFFER_SIZE: usize = 4096;

struct Connection {
    stream: TcpStream,
    buf: Vec<u8>,
    buf_len: usize,
    write_pos: usize,
}

impl Connection {
    fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            buf: vec![0u8; BUFFER_SIZE],
            buf_len: 0,
            write_pos: 0,
        }
    }

    fn read(&mut self) -> io::Result<bool> {
        loop {
            match self.stream.read(&mut self.buf[self.buf_len..]) {
                Ok(0) => return Ok(false), // Connection closed
                Ok(n) => {
                    self.buf_len += n;
                    if self.buf_len == self.buf.len() {
                        // Buffer full, need to write first
                        return Ok(true);
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    return Ok(true);
                }
                Err(e) => return Err(e),
            }
        }
    }

    fn write(&mut self) -> io::Result<bool> {
        while self.write_pos < self.buf_len {
            match self.stream.write(&self.buf[self.write_pos..self.buf_len]) {
                Ok(0) => return Ok(false), // Connection closed
                Ok(n) => {
                    self.write_pos += n;
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    return Ok(true);
                }
                Err(e) => return Err(e),
            }
        }

        // All data written, reset buffer
        self.buf_len = 0;
        self.write_pos = 0;
        Ok(true)
    }
}

fn main() -> io::Result<()> {
    let addr = "127.0.0.1:12345";
    let mut listener = TcpListener::bind(addr.parse().unwrap())?;
    println!("exec-rs mio echo server listening on {}", addr);

    let mut poll = Poll::new()?;
    let mut events = Events::with_capacity(1024);

    poll.registry()
        .register(&mut listener, SERVER, Interest::READABLE)?;

    let mut connections: HashMap<Token, Connection> = HashMap::new();
    let mut next_token = 1usize;

    let running = Arc::new(AtomicBool::new(true));

    while running.load(Ordering::Relaxed) {
        poll.poll(&mut events, Some(std::time::Duration::from_millis(100)))?;

        for event in events.iter() {
            match event.token() {
                SERVER => {
                    // Accept new connections
                    loop {
                        match listener.accept() {
                            Ok((mut stream, _addr)) => {
                                let token = Token(next_token);
                                next_token += 1;

                                poll.registry().register(
                                    &mut stream,
                                    token,
                                    Interest::READABLE | Interest::WRITABLE,
                                )?;

                                connections.insert(token, Connection::new(stream));
                            }
                            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                                break;
                            }
                            Err(e) => {
                                eprintln!("Accept error: {}", e);
                                break;
                            }
                        }
                    }
                }
                token => {
                    let mut remove = false;

                    if let Some(conn) = connections.get_mut(&token) {
                        if event.is_readable() {
                            match conn.read() {
                                Ok(true) => {}
                                Ok(false) | Err(_) => {
                                    remove = true;
                                }
                            }
                        }

                        if !remove && (event.is_writable() || conn.buf_len > conn.write_pos) {
                            match conn.write() {
                                Ok(true) => {}
                                Ok(false) | Err(_) => {
                                    remove = true;
                                }
                            }
                        }
                    }

                    if remove {
                        if let Some(mut conn) = connections.remove(&token) {
                            poll.registry().deregister(&mut conn.stream).ok();
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
