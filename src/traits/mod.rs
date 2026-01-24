//! Core trait definitions for the execution model.
//!
//! This module defines the fundamental traits that form the basis of the
//! sender/receiver model:
//!
//! - [`Receiver`]: Handles completion signals from senders
//! - [`Sender`]: Describes lazy asynchronous work
//! - [`OperationState`]: Represents connected sender-receiver pairs
//! - [`Scheduler`]: Creates senders that execute on specific contexts

mod completion;
mod receiver;
mod sender;
mod operation;
mod scheduler;
mod env;

pub use completion::*;
pub use receiver::*;
pub use sender::*;
pub use operation::*;
pub use scheduler::*;
pub use env::*;
