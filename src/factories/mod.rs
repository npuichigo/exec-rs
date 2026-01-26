//! Sender factories.
//!
//! This module provides functions to create basic senders:
//!
//! - [`just`]: Create a sender that completes with a value
//! - [`just_error`]: Create a sender that completes with an error
//! - [`just_stopped`]: Create a sender that completes with stopped
//! - [`just_from`]: Create a sender that calls a function to produce a value
//! - [`schedule`]: Create a sender from a scheduler
//! - [`transfer_just`]: Create a sender that completes with a value on a scheduler
//! - [`read_env`]: Create a sender that reads from the receiver's environment

mod just;
mod read_env;
mod schedule;

pub use just::*;
pub use read_env::*;
pub use schedule::*;
