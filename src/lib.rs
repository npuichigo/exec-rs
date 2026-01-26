//! # exec-rs
//!
//! A Rust implementation of C++ std::execution (P2300).
//!
//! This library provides lazy, composable asynchronous programming primitives
//! with zero-cost abstractions, following the design of the C++ P2300 proposal.
//!
//! ## Core Concepts
//!
//! - **Sender**: A lazy description of asynchronous work that produces values
//! - **Receiver**: A callback that handles completion signals (value, error, stopped)
//! - **OperationState**: The result of connecting a sender to a receiver
//! - **Scheduler**: A factory for senders that complete on a particular execution context
//!
//! ## Example
//!
//! ```rust,ignore
//! use exec_rs::prelude::*;
//!
//! let result = sync_wait(
//!     just(42)
//!         .then(|x| x * 2)
//!         .let_value(|x| just(format!("result: {}", x)))
//! ).unwrap();
//!
//! assert_eq!(result, "result: 84");
//! ```

#![warn(missing_docs)]
#![warn(rust_2018_idioms)]
#![allow(dead_code)]
#![allow(ambiguous_glob_reexports)]
#![deny(unsafe_op_in_unsafe_fn)]

pub mod adaptors;
pub mod any;
pub mod consumers;
pub mod coro;
pub mod factories;
pub mod future;
pub mod net;
pub mod schedulers;
pub mod scope;
pub mod sequence;
pub mod stop;
pub mod traits;
pub mod util;

/// Prelude module containing commonly used types and traits.
pub mod prelude {
    pub use crate::adaptors::*;
    pub use crate::consumers::*;
    pub use crate::factories::*;
    pub use crate::schedulers::InlineScheduler;
    pub use crate::stop::{
        InplaceStopSource, InplaceStopToken, NeverStopToken, StopCallback, StopSource, StopToken,
    };
    pub use crate::traits::*;
}
