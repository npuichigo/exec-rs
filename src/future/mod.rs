//! Future bridge - interop with std::future::Future.
//!
//! This module provides bidirectional conversion between exec-rs senders
//! and Rust's standard Future trait:
//!
//! - [`SenderToFutureExt`]: Convert a sender to a Future (`.into_future()`)
//! - [`FutureToSenderExt`]: Convert a Future to a sender (`.into_sender()`)
//! - [`from_future`]: Create a sender from a Future
//!
//! # Examples
//!
//! ## Sender to Future
//!
//! ```rust
//! use exec_rs::factories::just;
//! use exec_rs::future::SenderToFutureExt;
//!
//! # fn main() {
//! # // Using a simple block_on for the example
//! # let result = exec_rs::consumers::sync_wait(just(42)).unwrap();
//! # assert_eq!(result, 42);
//! # }
//! ```
//!
//! ## Future to Sender
//!
//! ```rust
//! use exec_rs::future::from_future;
//! use exec_rs::consumers::sync_wait;
//!
//! let sender = from_future(async { 42 });
//! let result = sync_wait(sender).unwrap();
//! assert_eq!(result, 42);
//! ```

mod future_to_sender;
mod sender_to_future;

pub use future_to_sender::{from_future, FutureSender, FutureToSenderExt};
pub use sender_to_future::{FutureReceiver, SenderFuture, SenderFutureError, SenderToFutureExt};
