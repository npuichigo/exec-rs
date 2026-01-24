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
#![deny(unsafe_op_in_unsafe_fn)]

pub mod traits;
pub mod stop;
pub mod factories;
pub mod adaptors;
pub mod consumers;
pub mod schedulers;
pub mod scope;
pub mod any;
pub mod coro;
pub mod util;
pub mod future;

/// Prelude module containing commonly used types and traits.
pub mod prelude {
    pub use crate::traits::*;
    pub use crate::stop::{
        StopToken, StopSource, StopCallback,
        InplaceStopSource, InplaceStopToken,
        NeverStopToken,
    };
    pub use crate::factories::*;
    pub use crate::adaptors::*;
    pub use crate::consumers::*;
    pub use crate::schedulers::InlineScheduler;
}
