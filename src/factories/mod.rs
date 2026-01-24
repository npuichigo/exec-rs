//! Sender factories.
//!
//! This module provides functions to create basic senders:
//!
//! - [`just`]: Create a sender that completes with a value
//! - [`just_error`]: Create a sender that completes with an error
//! - [`just_stopped`]: Create a sender that completes with stopped
//! - [`just_from`]: Create a sender that calls a function to produce a value

mod just;

pub use just::*;
