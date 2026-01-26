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
mod env;
mod operation;
mod receiver;
mod scheduler;
mod sender;

pub use completion::*;
pub use env::*;
pub use operation::*;
pub use receiver::*;
pub use scheduler::*;
pub use sender::*;
