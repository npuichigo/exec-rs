//! Buffer types for network I/O.
//!
//! Provides abstractions for read and write buffers used in async I/O operations.

use std::ops::{Deref, DerefMut};

/// A read-only buffer for sending data.
#[derive(Debug, Clone)]
pub struct Buffer<'a> {
    data: &'a [u8],
}

impl<'a> Buffer<'a> {
    /// Create a new buffer from a byte slice.
    pub fn new(data: &'a [u8]) -> Self {
        Self { data }
    }

    /// Get the length of the buffer.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Check if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

impl<'a> Deref for Buffer<'a> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.data
    }
}

impl<'a> From<&'a [u8]> for Buffer<'a> {
    fn from(data: &'a [u8]) -> Self {
        Self::new(data)
    }
}

impl<'a> From<&'a str> for Buffer<'a> {
    fn from(s: &'a str) -> Self {
        Self::new(s.as_bytes())
    }
}

impl<'a, const N: usize> From<&'a [u8; N]> for Buffer<'a> {
    fn from(data: &'a [u8; N]) -> Self {
        Self::new(data.as_slice())
    }
}

/// A mutable buffer for receiving data.
#[derive(Debug)]
pub struct MutBuffer<'a> {
    data: &'a mut [u8],
}

impl<'a> MutBuffer<'a> {
    /// Create a new mutable buffer from a byte slice.
    pub fn new(data: &'a mut [u8]) -> Self {
        Self { data }
    }

    /// Get the length of the buffer.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Check if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

impl<'a> Deref for MutBuffer<'a> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.data
    }
}

impl<'a> DerefMut for MutBuffer<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.data
    }
}

impl<'a> From<&'a mut [u8]> for MutBuffer<'a> {
    fn from(data: &'a mut [u8]) -> Self {
        Self::new(data)
    }
}

impl<'a, const N: usize> From<&'a mut [u8; N]> for MutBuffer<'a> {
    fn from(data: &'a mut [u8; N]) -> Self {
        Self::new(data.as_mut_slice())
    }
}

/// Create a read-only buffer from a byte slice.
pub fn buffer(data: &[u8]) -> Buffer<'_> {
    Buffer::new(data)
}

/// Create a mutable buffer from a byte slice.
pub fn mut_buffer(data: &mut [u8]) -> MutBuffer<'_> {
    MutBuffer::new(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_from_slice() {
        let data = [1u8, 2, 3, 4];
        let buf = Buffer::from(&data[..]);
        assert_eq!(buf.len(), 4);
        assert_eq!(&*buf, &[1, 2, 3, 4]);
    }

    #[test]
    fn test_buffer_from_str() {
        let buf = Buffer::from("hello");
        assert_eq!(buf.len(), 5);
        assert_eq!(&*buf, b"hello");
    }

    #[test]
    fn test_mut_buffer() {
        let mut data = [0u8; 4];
        let mut buf = MutBuffer::from(&mut data);
        buf[0] = 1;
        buf[1] = 2;
        assert_eq!(&*buf, &[1, 2, 0, 0]);
    }
}
