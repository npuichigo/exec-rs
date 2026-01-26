//! TCP socket types for async I/O.
//!
//! Provides `TcpListener` and `TcpStream` that integrate with `IoContext`.

use super::io_context::IoContext;
use mio::net as mio_net;
use mio::{Interest, Token};
use std::cell::UnsafeCell;
use std::io::{self, Read, Write};
use std::net::SocketAddr;
#[cfg(unix)]
use std::os::unix::io::{AsRawFd, RawFd};

/// A TCP listener for accepting incoming connections.
///
/// This is a wrapper around `mio::net::TcpListener` that integrates with
/// the `IoContext` for async operations.
pub struct TcpListener {
    inner: mio_net::TcpListener,
    token: Token,
    #[cfg(unix)]
    fd: RawFd,
}

impl TcpListener {
    /// Create a new TCP listener bound to the given address.
    pub fn bind(ctx: &IoContext, addr: &str) -> io::Result<Self> {
        let socket_addr: SocketAddr = addr
            .parse()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

        Self::bind_addr(ctx, socket_addr)
    }

    /// Create a new TCP listener bound to the given socket address.
    pub fn bind_addr(ctx: &IoContext, addr: SocketAddr) -> io::Result<Self> {
        let listener = mio_net::TcpListener::bind(addr)?;
        let token = ctx.allocate_token();

        #[cfg(unix)]
        let fd = listener.as_raw_fd();

        #[cfg(unix)]
        ctx.register_socket(token, fd, Interest::READABLE)?;

        Ok(Self {
            inner: listener,
            token,
            #[cfg(unix)]
            fd,
        })
    }

    /// Get the local address this listener is bound to.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.inner.local_addr()
    }

    /// Get the token for this listener.
    pub(crate) fn token(&self) -> Token {
        self.token
    }

    /// Accept a connection (non-blocking).
    ///
    /// Returns `Ok(Some((stream, addr)))` if a connection is available,
    /// `Ok(None)` if no connection is ready (would block),
    /// or an error.
    pub fn accept_nonblocking(
        &self,
        ctx: &IoContext,
    ) -> io::Result<Option<(TcpStream, SocketAddr)>> {
        match self.inner.accept() {
            Ok((stream, addr)) => {
                let tcp_stream = TcpStream::from_mio(ctx, stream)?;
                Ok(Some((tcp_stream, addr)))
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Get the underlying mio listener (for internal use).
    pub(crate) fn mio_listener(&self) -> &mio_net::TcpListener {
        &self.inner
    }
}

#[cfg(unix)]
impl AsRawFd for TcpListener {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}

/// A TCP stream for bidirectional communication.
///
/// This is a wrapper around `mio::net::TcpStream` that integrates with
/// the `IoContext` for async operations.
///
/// Uses `UnsafeCell` for interior mutability to allow reading/writing
/// from shared references in async operations.
pub struct TcpStream {
    inner: UnsafeCell<mio_net::TcpStream>,
    token: Token,
    #[cfg(unix)]
    fd: RawFd,
}

impl TcpStream {
    /// Connect to a remote address.
    pub fn connect(ctx: &IoContext, addr: &str) -> io::Result<Self> {
        let socket_addr: SocketAddr = addr
            .parse()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

        Self::connect_addr(ctx, socket_addr)
    }

    /// Connect to a remote socket address.
    pub fn connect_addr(ctx: &IoContext, addr: SocketAddr) -> io::Result<Self> {
        let stream = mio_net::TcpStream::connect(addr)?;
        Self::from_mio(ctx, stream)
    }

    /// Create a TcpStream from a mio TcpStream.
    pub(crate) fn from_mio(ctx: &IoContext, stream: mio_net::TcpStream) -> io::Result<Self> {
        let token = ctx.allocate_token();

        #[cfg(unix)]
        let fd = stream.as_raw_fd();

        #[cfg(unix)]
        ctx.register_socket(token, fd, Interest::READABLE | Interest::WRITABLE)?;

        Ok(Self {
            inner: UnsafeCell::new(stream),
            token,
            #[cfg(unix)]
            fd,
        })
    }

    /// Get the peer address.
    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        // SAFETY: peer_addr only reads from the stream
        unsafe { (*self.inner.get()).peer_addr() }
    }

    /// Get the local address.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        // SAFETY: local_addr only reads from the stream
        unsafe { (*self.inner.get()).local_addr() }
    }

    /// Get the token for this stream.
    pub(crate) fn token(&self) -> Token {
        self.token
    }

    /// Read from the stream (non-blocking).
    ///
    /// Returns `Ok(Some(n))` if n bytes were read,
    /// `Ok(None)` if the operation would block,
    /// or an error.
    pub fn read_nonblocking(&self, buf: &mut [u8]) -> io::Result<Option<usize>> {
        // SAFETY: We're the only accessor at this point (single-threaded IoContext)
        let inner = unsafe { &mut *self.inner.get() };
        match inner.read(buf) {
            Ok(n) => Ok(Some(n)),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Write to the stream (non-blocking).
    ///
    /// Returns `Ok(Some(n))` if n bytes were written,
    /// `Ok(None)` if the operation would block,
    /// or an error.
    pub fn write_nonblocking(&self, buf: &[u8]) -> io::Result<Option<usize>> {
        // SAFETY: We're the only accessor at this point (single-threaded IoContext)
        let inner = unsafe { &mut *self.inner.get() };
        match inner.write(buf) {
            Ok(n) => Ok(Some(n)),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Get a mutable reference to the underlying mio stream.
    ///
    /// # Safety
    ///
    /// Caller must ensure exclusive access to the stream.
    pub(crate) unsafe fn mio_stream_mut(&self) -> &mut mio_net::TcpStream {
        // SAFETY: Caller guarantees exclusive access
        unsafe { &mut *self.inner.get() }
    }
}

impl Read for TcpStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // SAFETY: We have &mut self, so exclusive access
        unsafe { (*self.inner.get()).read(buf) }
    }
}

impl Write for TcpStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // SAFETY: We have &mut self, so exclusive access
        unsafe { (*self.inner.get()).write(buf) }
    }

    fn flush(&mut self) -> io::Result<()> {
        // SAFETY: We have &mut self, so exclusive access
        unsafe { (*self.inner.get()).flush() }
    }
}

#[cfg(unix)]
impl AsRawFd for TcpStream {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tcp_listener_bind() {
        let ctx = IoContext::new().unwrap();
        let listener = TcpListener::bind(&ctx, "127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        assert!(addr.port() > 0);
    }
}
