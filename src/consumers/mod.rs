//! Sender consumers.
//!
//! Consumers are the terminal operations that actually execute senders.
//!
//! - [`sync_wait`]: Block until a sender completes
//! - [`start_detached`]: Fire-and-forget execution

mod start_detached;
mod sync_wait;

pub use start_detached::*;
pub use sync_wait::*;
