//! Networking module for exec-rs.
//!
//! This module provides async networking primitives built on mio and integrated
//! with the sender/receiver model from P2300/P2762.
//!
//! # Architecture
//!
//! - `IoContext`: The async I/O runtime using mio for event notification
//! - `TcpListener`: Async TCP server socket for accepting connections
//! - `TcpStream`: Async TCP stream for bidirectional communication
//! - `async_accept`, `async_read`, `async_write`: Sender-based async operations
//!
//! # Example
//!
//! ```rust,ignore
//! use exec_rs::net::{IoContext, TcpListener, async_accept, async_read, async_write};
//! use exec_rs::consumers::sync_wait;
//!
//! let mut ctx = IoContext::new().unwrap();
//! let listener = TcpListener::bind(&mut ctx, "127.0.0.1:12345").unwrap();
//!
//! // Accept a connection
//! let accept_sender = async_accept(&listener);
//! // ... run with ctx.run()
//! ```

mod buffer;
mod io_context;
mod operations;
mod socket;

pub use buffer::{Buffer, MutBuffer};
pub use io_context::{IoContext, IoContextScheduler};
pub use operations::{
    async_accept, async_accept_arc, async_receive, async_receive_into, async_send,
    async_send_from, AsyncAccept, AsyncAcceptArc, AsyncReceive, AsyncReceiveInto, AsyncSend,
    AsyncSendFrom, ClientState,
};
pub use socket::{TcpListener, TcpStream};
