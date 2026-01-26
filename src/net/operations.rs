//! Async I/O operations as senders.
//!
//! This module provides sender-based async operations for networking
//! following the P2762 proposal naming:
//! - `async_accept`: Accept an incoming connection
//! - `async_receive`: Receive data from a stream
//! - `async_send`: Send data to a stream
//!
//! These operations return senders that complete when the I/O is ready,
//! integrating with the IoContext event loop.

use super::io_context::{IoContext, IoEvent};
use super::socket::{TcpListener, TcpStream};
use crate::traits::{CompletionSignatures, OperationState, ReceiverOf, ReceiverOfError, Sender, SenderTo};
use mio::Interest;
use std::cell::UnsafeCell;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;

// ============================================================================
// AsyncAccept - Accept a connection from a TCP listener
// ============================================================================

/// Create a sender that accepts a connection from a TCP listener.
///
/// The sender completes with `(TcpStream, SocketAddr)` when a connection is accepted.
///
/// # Example
///
/// ```rust,ignore
/// let ctx = IoContext::new()?;
/// let listener = TcpListener::bind(&ctx, "127.0.0.1:12345")?;
///
/// // Using run_sender:
/// let (stream, addr) = ctx.run_sender(async_accept(&listener, &ctx))?;
///
/// // Or with sender composition:
/// let server = async_accept(&listener, &ctx)
///     .let_value(|(stream, addr)| handle_client(stream, addr));
/// ```
pub fn async_accept<'a>(listener: &'a TcpListener, ctx: &'a IoContext) -> AsyncAccept<'a> {
    AsyncAccept { listener, ctx }
}

/// Sender for async accept operation.
pub struct AsyncAccept<'a> {
    listener: &'a TcpListener,
    ctx: &'a IoContext,
}

impl<'a> Clone for AsyncAccept<'a> {
    fn clone(&self) -> Self {
        Self {
            listener: self.listener,
            ctx: self.ctx,
        }
    }
}

/// Completion signatures for async accept.
pub struct AsyncAcceptCompletions;

impl CompletionSignatures for AsyncAcceptCompletions {
    type Value = (TcpStream, SocketAddr);
    type Error = io::Error;
    const SENDS_STOPPED: bool = false;
}

impl<'a> Sender for AsyncAccept<'a> {
    type Completions = AsyncAcceptCompletions;
}

impl<'a, R> SenderTo<R> for AsyncAccept<'a>
where
    R: ReceiverOf<(TcpStream, SocketAddr)> + ReceiverOfError<io::Error> + 'static,
{
    type Operation = AsyncAcceptOperation<'a, R>;

    fn connect(self, receiver: R) -> Self::Operation {
        AsyncAcceptOperation {
            listener: self.listener,
            ctx: self.ctx,
            receiver: UnsafeCell::new(Some(receiver)),
        }
    }
}

/// Operation state for async accept.
pub struct AsyncAcceptOperation<'a, R> {
    listener: &'a TcpListener,
    ctx: &'a IoContext,
    receiver: UnsafeCell<Option<R>>,
}

impl<'a, R> OperationState for AsyncAcceptOperation<'a, R>
where
    R: ReceiverOf<(TcpStream, SocketAddr)> + ReceiverOfError<io::Error> + 'static,
{
    fn start(self: Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };

        // Try to accept immediately (non-blocking).
        match this.listener.accept_nonblocking(this.ctx) {
            Ok(Some((stream, addr))) => {
                // Accepted immediately - complete with value.
                if let Some(receiver) = unsafe { (*this.receiver.get()).take() } {
                    receiver.set_value((stream, addr));
                }
            }
            Ok(None) => {
                // Would block - register for readiness notification.
                let token = this.listener.token();
                let receiver = unsafe { (*this.receiver.get()).take() };
                let listener_ptr = this.listener as *const TcpListener;
                let ctx_ptr = this.ctx as *const IoContext;

                if let Some(receiver) = receiver {
                    let completion = Box::new(move |result: io::Result<IoEvent>| {
                        match result {
                            Ok(_event) => {
                                // Socket is ready - try accept again.
                                let listener = unsafe { &*listener_ptr };
                                let ctx = unsafe { &*ctx_ptr };

                                match listener.accept_nonblocking(ctx) {
                                    Ok(Some((stream, addr))) => {
                                        receiver.set_value((stream, addr));
                                    }
                                    Ok(None) => {
                                        // Spurious wakeup
                                        receiver.set_error(io::Error::new(
                                            io::ErrorKind::WouldBlock,
                                            "spurious wakeup, retry accept",
                                        ));
                                    }
                                    Err(e) => {
                                        receiver.set_error(e);
                                    }
                                }
                            }
                            Err(e) => {
                                receiver.set_error(e);
                            }
                        }
                    });

                    this.ctx.submit_io(token, Interest::READABLE, completion);
                }
            }
            Err(e) => {
                if let Some(receiver) = unsafe { (*this.receiver.get()).take() } {
                    receiver.set_error(e);
                }
            }
        }
    }
}

// ============================================================================
// AsyncAcceptArc - Arc-based accept for use with repeat_effect
// ============================================================================

/// Create a sender that accepts a connection using Arc-wrapped listener.
///
/// This Arc-based version can be used in closures for sender composition
/// with `repeat_effect`.
///
/// # Example
///
/// ```rust,ignore
/// use exec_rs::adaptors::{repeat_effect, then};
/// use exec_rs::net::{async_accept_arc, IoContext, TcpListener};
///
/// let ctx = Arc::new(IoContext::new()?);
/// let listener = Arc::new(TcpListener::bind(&ctx, "127.0.0.1:12345")?);
///
/// let accept_loop = repeat_effect(move || {
///     let listener = listener.clone();
///     let ctx = ctx.clone();
///     then(async_accept_arc(listener, ctx), |(stream, addr)| {
///         println!("Accepted from {}", addr);
///         // spawn client handler...
///     })
/// });
/// ```
pub fn async_accept_arc(
    listener: Arc<TcpListener>,
    ctx: Arc<IoContext>,
) -> AsyncAcceptArc {
    AsyncAcceptArc { listener, ctx }
}

/// Sender for Arc-based async accept operation.
pub struct AsyncAcceptArc {
    listener: Arc<TcpListener>,
    ctx: Arc<IoContext>,
}

impl Clone for AsyncAcceptArc {
    fn clone(&self) -> Self {
        Self {
            listener: self.listener.clone(),
            ctx: self.ctx.clone(),
        }
    }
}

impl Sender for AsyncAcceptArc {
    type Completions = AsyncAcceptCompletions;
}

impl<R> SenderTo<R> for AsyncAcceptArc
where
    R: ReceiverOf<(TcpStream, SocketAddr)> + ReceiverOfError<io::Error> + 'static,
{
    type Operation = AsyncAcceptArcOperation<R>;

    fn connect(self, receiver: R) -> Self::Operation {
        AsyncAcceptArcOperation {
            listener: self.listener,
            ctx: self.ctx,
            receiver: UnsafeCell::new(Some(receiver)),
        }
    }
}

/// Operation state for Arc-based async accept.
pub struct AsyncAcceptArcOperation<R> {
    listener: Arc<TcpListener>,
    ctx: Arc<IoContext>,
    receiver: UnsafeCell<Option<R>>,
}

unsafe impl<R: Send> Send for AsyncAcceptArcOperation<R> {}

impl<R> OperationState for AsyncAcceptArcOperation<R>
where
    R: ReceiverOf<(TcpStream, SocketAddr)> + ReceiverOfError<io::Error> + 'static,
{
    fn start(self: Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };

        // Try to accept immediately (non-blocking).
        match this.listener.accept_nonblocking(&this.ctx) {
            Ok(Some((stream, addr))) => {
                // Accepted immediately - complete with value.
                if let Some(receiver) = unsafe { (*this.receiver.get()).take() } {
                    receiver.set_value((stream, addr));
                }
            }
            Ok(None) => {
                // Would block - register for readiness notification.
                let token = this.listener.token();
                let receiver = unsafe { (*this.receiver.get()).take() };
                let listener = this.listener.clone();
                let ctx = this.ctx.clone();

                if let Some(receiver) = receiver {
                    let completion = Box::new(move |result: io::Result<IoEvent>| match result {
                        Ok(_event) => {
                            match listener.accept_nonblocking(&ctx) {
                                Ok(Some((stream, addr))) => {
                                    receiver.set_value((stream, addr));
                                }
                                Ok(None) => {
                                    // Spurious wakeup
                                    receiver.set_error(io::Error::new(
                                        io::ErrorKind::WouldBlock,
                                        "spurious wakeup, retry accept",
                                    ));
                                }
                                Err(e) => {
                                    receiver.set_error(e);
                                }
                            }
                        }
                        Err(e) => {
                            receiver.set_error(e);
                        }
                    });

                    this.ctx.submit_io(token, Interest::READABLE, completion);
                }
            }
            Err(e) => {
                if let Some(receiver) = unsafe { (*this.receiver.get()).take() } {
                    receiver.set_error(e);
                }
            }
        }
    }
}

// ============================================================================
// AsyncReceive - Receive data from a TCP stream
// ============================================================================

/// Create a sender that receives data from a TCP stream into a buffer.
///
/// The sender completes with `usize` indicating the number of bytes received.
/// Returns 0 when the connection is closed (EOF).
///
/// # Example
///
/// ```rust,ignore
/// let mut buffer = [0u8; 1024];
/// let n = ctx.run_sender(async_receive(&stream, &ctx, &mut buffer))?;
/// println!("Received {} bytes", n);
/// ```
pub fn async_receive<'a>(
    stream: &'a TcpStream,
    ctx: &'a IoContext,
    buffer: &'a mut [u8],
) -> AsyncReceive<'a> {
    AsyncReceive { stream, ctx, buffer }
}

/// Sender for async receive operation.
pub struct AsyncReceive<'a> {
    stream: &'a TcpStream,
    ctx: &'a IoContext,
    buffer: &'a mut [u8],
}

/// Completion signatures for async receive.
pub struct AsyncReceiveCompletions;

impl CompletionSignatures for AsyncReceiveCompletions {
    type Value = usize;
    type Error = io::Error;
    const SENDS_STOPPED: bool = false;
}

impl<'a> Sender for AsyncReceive<'a> {
    type Completions = AsyncReceiveCompletions;
}

impl<'a, R> SenderTo<R> for AsyncReceive<'a>
where
    R: ReceiverOf<usize> + ReceiverOfError<io::Error> + 'static,
{
    type Operation = AsyncReceiveOperation<'a, R>;

    fn connect(self, receiver: R) -> Self::Operation {
        AsyncReceiveOperation {
            stream: self.stream,
            ctx: self.ctx,
            buffer: self.buffer,
            receiver: UnsafeCell::new(Some(receiver)),
        }
    }
}

/// Operation state for async receive.
pub struct AsyncReceiveOperation<'a, R> {
    stream: &'a TcpStream,
    ctx: &'a IoContext,
    buffer: &'a mut [u8],
    receiver: UnsafeCell<Option<R>>,
}

impl<'a, R> OperationState for AsyncReceiveOperation<'a, R>
where
    R: ReceiverOf<usize> + ReceiverOfError<io::Error> + 'static,
{
    fn start(self: Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };

        // Try to read immediately using interior mutability.
        match this.stream.read_nonblocking(this.buffer) {
            Ok(Some(n)) => {
                if let Some(receiver) = unsafe { (*this.receiver.get()).take() } {
                    receiver.set_value(n);
                }
            }
            Ok(None) => {
                // Would block - register for readiness.
                let token = this.stream.token();
                let receiver = unsafe { (*this.receiver.get()).take() };
                let stream_ptr = this.stream as *const TcpStream;
                let buffer_ptr = this.buffer.as_mut_ptr();
                let buffer_len = this.buffer.len();

                if let Some(receiver) = receiver {
                    let completion = Box::new(move |result: io::Result<IoEvent>| match result {
                        Ok(_event) => {
                            // Socket is ready - try read again.
                            let stream = unsafe { &*stream_ptr };
                            let buffer = unsafe { std::slice::from_raw_parts_mut(buffer_ptr, buffer_len) };

                            match stream.read_nonblocking(buffer) {
                                Ok(Some(n)) => {
                                    receiver.set_value(n);
                                }
                                Ok(None) => {
                                    // Still would block - spurious wakeup
                                    receiver.set_error(io::Error::new(
                                        io::ErrorKind::WouldBlock,
                                        "spurious wakeup",
                                    ));
                                }
                                Err(e) => {
                                    receiver.set_error(e);
                                }
                            }
                        }
                        Err(e) => {
                            receiver.set_error(e);
                        }
                    });

                    this.ctx.submit_io(token, Interest::READABLE, completion);
                }
            }
            Err(e) => {
                if let Some(receiver) = unsafe { (*this.receiver.get()).take() } {
                    receiver.set_error(e);
                }
            }
        }
    }
}

// ============================================================================
// AsyncSend - Send data to a TCP stream
// ============================================================================

/// Create a sender that sends data to a TCP stream.
///
/// The sender completes with `usize` indicating the number of bytes sent.
///
/// # Example
///
/// ```rust,ignore
/// let data = b"Hello, World!";
/// let n = ctx.run_sender(async_send(&stream, &ctx, data))?;
/// println!("Sent {} bytes", n);
/// ```
pub fn async_send<'a>(
    stream: &'a TcpStream,
    ctx: &'a IoContext,
    buffer: &'a [u8],
) -> AsyncSend<'a> {
    AsyncSend { stream, ctx, buffer }
}

/// Sender for async send operation.
pub struct AsyncSend<'a> {
    stream: &'a TcpStream,
    ctx: &'a IoContext,
    buffer: &'a [u8],
}

/// Completion signatures for async send.
pub struct AsyncSendCompletions;

impl CompletionSignatures for AsyncSendCompletions {
    type Value = usize;
    type Error = io::Error;
    const SENDS_STOPPED: bool = false;
}

impl<'a> Sender for AsyncSend<'a> {
    type Completions = AsyncSendCompletions;
}

impl<'a, R> SenderTo<R> for AsyncSend<'a>
where
    R: ReceiverOf<usize> + ReceiverOfError<io::Error> + 'static,
{
    type Operation = AsyncSendOperation<'a, R>;

    fn connect(self, receiver: R) -> Self::Operation {
        AsyncSendOperation {
            stream: self.stream,
            ctx: self.ctx,
            buffer: self.buffer,
            receiver: UnsafeCell::new(Some(receiver)),
        }
    }
}

/// Operation state for async send.
pub struct AsyncSendOperation<'a, R> {
    stream: &'a TcpStream,
    ctx: &'a IoContext,
    buffer: &'a [u8],
    receiver: UnsafeCell<Option<R>>,
}

impl<'a, R> OperationState for AsyncSendOperation<'a, R>
where
    R: ReceiverOf<usize> + ReceiverOfError<io::Error> + 'static,
{
    fn start(self: Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };

        // Try to write immediately using interior mutability.
        match this.stream.write_nonblocking(this.buffer) {
            Ok(Some(n)) => {
                if let Some(receiver) = unsafe { (*this.receiver.get()).take() } {
                    receiver.set_value(n);
                }
            }
            Ok(None) => {
                // Would block - register for readiness.
                let token = this.stream.token();
                let receiver = unsafe { (*this.receiver.get()).take() };
                let stream_ptr = this.stream as *const TcpStream;
                let buffer_ptr = this.buffer.as_ptr();
                let buffer_len = this.buffer.len();

                if let Some(receiver) = receiver {
                    let completion = Box::new(move |result: io::Result<IoEvent>| match result {
                        Ok(_event) => {
                            // Socket is ready - try write again.
                            let stream = unsafe { &*stream_ptr };
                            let buffer = unsafe { std::slice::from_raw_parts(buffer_ptr, buffer_len) };

                            match stream.write_nonblocking(buffer) {
                                Ok(Some(n)) => {
                                    receiver.set_value(n);
                                }
                                Ok(None) => {
                                    // Still would block - spurious wakeup
                                    receiver.set_error(io::Error::new(
                                        io::ErrorKind::WouldBlock,
                                        "spurious wakeup",
                                    ));
                                }
                                Err(e) => {
                                    receiver.set_error(e);
                                }
                            }
                        }
                        Err(e) => {
                            receiver.set_error(e);
                        }
                    });

                    this.ctx.submit_io(token, Interest::WRITABLE, completion);
                }
            }
            Err(e) => {
                if let Some(receiver) = unsafe { (*this.receiver.get()).take() } {
                    receiver.set_error(e);
                }
            }
        }
    }
}

// ============================================================================
// Arc-based operations for use in closures and sender composition
// ============================================================================

/// Shared client state for echo loop pattern.
///
/// This enables the kuhllib pattern where client state is shared across
/// multiple async operations in a sender chain.
pub struct ClientState {
    /// The TCP stream.
    pub stream: TcpStream,
    /// Buffer for read/write operations.
    pub buffer: UnsafeCell<Vec<u8>>,
    /// Number of bytes in buffer from last read.
    pub bytes_read: std::cell::Cell<usize>,
    /// Done flag for repeat_effect_until predicate.
    pub done: std::cell::Cell<bool>,
}

// SAFETY: ClientState is used in single-threaded IoContext
unsafe impl Send for ClientState {}
unsafe impl Sync for ClientState {}

impl ClientState {
    /// Create new client state with the given buffer size.
    pub fn new(stream: TcpStream, buffer_size: usize) -> Arc<Self> {
        Arc::new(Self {
            stream,
            buffer: UnsafeCell::new(vec![0u8; buffer_size]),
            bytes_read: std::cell::Cell::new(0),
            done: std::cell::Cell::new(false),
        })
    }

    /// Get a mutable slice of the buffer.
    ///
    /// # Safety
    /// Caller must ensure no concurrent access.
    pub unsafe fn buffer_mut(&self) -> &mut [u8] {
        // SAFETY: Caller guarantees no concurrent access
        unsafe { &mut *self.buffer.get() }
    }

    /// Get an immutable slice of the buffer up to bytes_read.
    ///
    /// # Safety
    /// Caller must ensure no concurrent writes.
    pub unsafe fn buffer_slice(&self) -> &[u8] {
        // SAFETY: Caller guarantees no concurrent writes
        let buf = unsafe { &*self.buffer.get() };
        &buf[..self.bytes_read.get()]
    }
}

/// Create a sender that receives data into a ClientState's buffer.
///
/// This Arc-based version can be used in closures for sender composition.
pub fn async_receive_into(client: Arc<ClientState>, ctx: Arc<IoContext>) -> AsyncReceiveInto {
    AsyncReceiveInto { client, ctx }
}

/// Sender for async receive into ClientState.
pub struct AsyncReceiveInto {
    client: Arc<ClientState>,
    ctx: Arc<IoContext>,
}

impl Clone for AsyncReceiveInto {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            ctx: self.ctx.clone(),
        }
    }
}

impl Sender for AsyncReceiveInto {
    type Completions = AsyncReceiveCompletions;
}

impl<R> SenderTo<R> for AsyncReceiveInto
where
    R: ReceiverOf<usize> + ReceiverOfError<io::Error> + 'static,
{
    type Operation = AsyncReceiveIntoOperation<R>;

    fn connect(self, receiver: R) -> Self::Operation {
        AsyncReceiveIntoOperation {
            client: self.client,
            ctx: self.ctx,
            receiver: UnsafeCell::new(Some(receiver)),
        }
    }
}

/// Operation state for async receive into ClientState.
pub struct AsyncReceiveIntoOperation<R> {
    client: Arc<ClientState>,
    ctx: Arc<IoContext>,
    receiver: UnsafeCell<Option<R>>,
}

unsafe impl<R: Send> Send for AsyncReceiveIntoOperation<R> {}

impl<R> OperationState for AsyncReceiveIntoOperation<R>
where
    R: ReceiverOf<usize> + ReceiverOfError<io::Error> + 'static,
{
    fn start(self: Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };

        let buffer = unsafe { this.client.buffer_mut() };
        match this.client.stream.read_nonblocking(buffer) {
            Ok(Some(n)) => {
                this.client.bytes_read.set(n);
                if let Some(receiver) = unsafe { (*this.receiver.get()).take() } {
                    receiver.set_value(n);
                }
            }
            Ok(None) => {
                let token = this.client.stream.token();
                let receiver = unsafe { (*this.receiver.get()).take() };
                let client = this.client.clone();

                if let Some(receiver) = receiver {
                    let completion = Box::new(move |result: io::Result<IoEvent>| match result {
                        Ok(_event) => {
                            let buffer = unsafe { client.buffer_mut() };
                            match client.stream.read_nonblocking(buffer) {
                                Ok(Some(n)) => {
                                    client.bytes_read.set(n);
                                    receiver.set_value(n);
                                }
                                Ok(None) => {
                                    receiver.set_error(io::Error::new(
                                        io::ErrorKind::WouldBlock,
                                        "spurious wakeup",
                                    ));
                                }
                                Err(e) => receiver.set_error(e),
                            }
                        }
                        Err(e) => receiver.set_error(e),
                    });

                    this.ctx.submit_io(token, Interest::READABLE, completion);
                }
            }
            Err(e) => {
                if let Some(receiver) = unsafe { (*this.receiver.get()).take() } {
                    receiver.set_error(e);
                }
            }
        }
    }
}

/// Create a sender that sends data from a ClientState's buffer.
///
/// This Arc-based version can be used in closures for sender composition.
pub fn async_send_from(client: Arc<ClientState>, ctx: Arc<IoContext>) -> AsyncSendFrom {
    AsyncSendFrom { client, ctx }
}

/// Sender for async send from ClientState.
pub struct AsyncSendFrom {
    client: Arc<ClientState>,
    ctx: Arc<IoContext>,
}

impl Clone for AsyncSendFrom {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            ctx: self.ctx.clone(),
        }
    }
}

impl Sender for AsyncSendFrom {
    type Completions = AsyncSendCompletions;
}

impl<R> SenderTo<R> for AsyncSendFrom
where
    R: ReceiverOf<usize> + ReceiverOfError<io::Error> + 'static,
{
    type Operation = AsyncSendFromOperation<R>;

    fn connect(self, receiver: R) -> Self::Operation {
        AsyncSendFromOperation {
            client: self.client,
            ctx: self.ctx,
            receiver: UnsafeCell::new(Some(receiver)),
        }
    }
}

/// Operation state for async send from ClientState.
pub struct AsyncSendFromOperation<R> {
    client: Arc<ClientState>,
    ctx: Arc<IoContext>,
    receiver: UnsafeCell<Option<R>>,
}

unsafe impl<R: Send> Send for AsyncSendFromOperation<R> {}

impl<R> OperationState for AsyncSendFromOperation<R>
where
    R: ReceiverOf<usize> + ReceiverOfError<io::Error> + 'static,
{
    fn start(self: Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };

        let buffer = unsafe { this.client.buffer_slice() };
        match this.client.stream.write_nonblocking(buffer) {
            Ok(Some(n)) => {
                if let Some(receiver) = unsafe { (*this.receiver.get()).take() } {
                    receiver.set_value(n);
                }
            }
            Ok(None) => {
                let token = this.client.stream.token();
                let receiver = unsafe { (*this.receiver.get()).take() };
                let client = this.client.clone();

                if let Some(receiver) = receiver {
                    let completion = Box::new(move |result: io::Result<IoEvent>| match result {
                        Ok(_event) => {
                            let buffer = unsafe { client.buffer_slice() };
                            match client.stream.write_nonblocking(buffer) {
                                Ok(Some(n)) => receiver.set_value(n),
                                Ok(None) => {
                                    receiver.set_error(io::Error::new(
                                        io::ErrorKind::WouldBlock,
                                        "spurious wakeup",
                                    ));
                                }
                                Err(e) => receiver.set_error(e),
                            }
                        }
                        Err(e) => receiver.set_error(e),
                    });

                    this.ctx.submit_io(token, Interest::WRITABLE, completion);
                }
            }
            Err(e) => {
                if let Some(receiver) = unsafe { (*this.receiver.get()).take() } {
                    receiver.set_error(e);
                }
            }
        }
    }
}
