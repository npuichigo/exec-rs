//! Cancellation subsystem with stop tokens.
//!
//! The stop token mechanism provides cooperative cancellation for asynchronous
//! operations. It consists of:
//!
//! - [`StopSource`]: The source of cancellation requests
//! - [`StopToken`]: A token that can be queried for stop requests
//! - [`StopCallback`]: A callback that runs when stop is requested
//!
//! # Example
//!
//! ```rust
//! use exec_rs::stop::{InplaceStopSource, StopSource, StopToken, StopCallback};
//!
//! let source = InplaceStopSource::new();
//! let token = source.get_token();
//!
//! // Register a callback
//! let called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
//! let called_clone = called.clone();
//! let _callback = token.register_callback(move || {
//!     called_clone.store(true, std::sync::atomic::Ordering::SeqCst);
//! });
//!
//! assert!(!token.stop_requested());
//!
//! // Request stop
//! source.request_stop();
//!
//! assert!(token.stop_requested());
//! assert!(called.load(std::sync::atomic::Ordering::SeqCst));
//! ```

mod token;
mod source;
mod inplace;
mod callback;
mod never;

pub use token::*;
pub use source::*;
pub use inplace::*;
pub use callback::*;
pub use never::*;
