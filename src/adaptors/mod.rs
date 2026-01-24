//! Sender adaptors.
//!
//! Adaptors transform senders by modifying their behavior or composing them
//! with other operations.
//!
//! ## Transformation Adaptors
//!
//! - [`Then`]: Transform the value with a function
//! - [`UponError`]: Transform the error with a function
//! - [`UponStopped`]: Transform stopped into a value
//! - [`IntoTuple`]: Wrap value in a single-element tuple
//! - [`Bulk`]: Execute a function for each index in a range
//!
//! ## Chaining Adaptors
//!
//! - [`LetValue`]: Chain with a function that returns a sender
//! - [`LetError`]: Handle errors with a sender-returning function
//! - [`LetStopped`]: Handle stopped with a sender-returning function
//!
//! ## Scheduling Adaptors
//!
//! - [`On`]: Run a sender on a specific scheduler
//! - [`StartsOn`]: Start execution on a specific scheduler
//! - [`ContinuesOn`]: Continue execution on a specific scheduler
//! - [`Reschedule`]: Reschedule on the environment's scheduler
//!
//! ## Concurrent Composition
//!
//! - [`WhenAll`]: Wait for multiple senders to complete
//! - [`WhenAny`]: Race multiple senders, first to complete wins
//!
//! ## Sharing and Eager Execution
//!
//! - [`Split`]: Convert single-shot sender to multi-shot (shared result)
//! - [`EnsureStarted`]: Start sender eagerly, get result later
//!
//! ## Stopped Handling
//!
//! - [`StoppedAsOptional`]: Transform stopped to `None`, values to `Some`
//! - [`StoppedAsError`]: Transform stopped to an error
//! - [`UnlessStopped`]: Check stop token and complete with stopped if requested
//!
//! ## Cleanup
//!
//! - [`Finally`]: Run cleanup sender after main sender completes
//!
//! ## Repetition
//!
//! - [`RepeatN`]: Repeat a sender N times
//! - [`RepeatEffectUntil`]: Repeat until the sender returns true
//!
//! ## Materialization
//!
//! - [`Materialize`]: Convert all completions to a `Completion` enum value

mod bulk;
mod then;
mod upon_error;
mod upon_stopped;
mod let_value;
mod let_error;
mod let_stopped;
mod on;
mod starts_on;
mod continues_on;
mod ensure_started;
mod when_all;
mod when_any;
mod stopped_as_optional;
mod stopped_as_error;
mod into_tuple;
mod finally;
mod materialize;
mod repeat;
mod reschedule;
mod split;
mod unless_stopped;

pub use bulk::*;
pub use then::*;
pub use upon_error::*;
pub use upon_stopped::*;
pub use let_value::*;
pub use let_error::*;
pub use let_stopped::*;
pub use on::*;
pub use starts_on::*;
pub use continues_on::*;
pub use ensure_started::*;
pub use when_all::*;
pub use when_any::*;
pub use stopped_as_optional::*;
pub use stopped_as_error::*;
pub use into_tuple::*;
pub use finally::*;
pub use materialize::*;
pub use repeat::*;
pub use reschedule::*;
pub use split::*;
pub use unless_stopped::*;
