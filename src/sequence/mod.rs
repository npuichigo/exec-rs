//! Sequence Senders (Streaming)
//!
//! This module implements multi-value senders that can emit multiple items over time,
//! similar to async streams or iterators.
//!
//! # Key Concepts
//!
//! - **SequenceSender**: A sender that produces multiple items before completing
//! - **SequenceReceiver**: Receives items via `set_next(item_sender)`
//! - **subscribe**: Like `connect` but for sequence senders
//!
//! # Example
//!
//! ```ignore
//! use exec_rs::sequence::{iterate, ignore_all_values};
//! use exec_rs::consumers::sync_wait;
//!
//! // Iterate over a range and ignore all values
//! let result = sync_wait(ignore_all_values(iterate(0..10)));
//! ```

mod empty;
mod ignore_all;
mod iterate;
mod traits;
mod transform_each;

pub use empty::{empty_sequence, EmptySequence};
pub use ignore_all::{ignore_all_values, IgnoreAllValues, IgnoreAllValuesExt};
pub use iterate::{iterate, Iterate};
pub use traits::{
    ItemTypes, NextSender, SequenceReceiver, SequenceReceiverOf, SequenceSender, SequenceSenderTo,
    SetNextResult,
};
pub use transform_each::{transform_each, TransformEach, TransformEachExt};
