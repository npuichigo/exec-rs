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
//! - [`RepeatEffect`]: Repeat a sender forever (until error/stopped)
//! - [`RepeatN`]: Repeat a sender N times
//! - [`RepeatEffectUntil`]: Repeat until the sender returns true
//!
//! ## Materialization
//!
//! - [`Materialize`]: Convert all completions to a `Completion` enum value
//!
//! ## Variant Wrapping
//!
//! - [`IntoVariant`]: Wrap value completion in `Option<T>`
//!
//! ## Environment Modification
//!
//! - [`WriteEnv`]: Compose a new environment with the receiver's environment

mod bulk;
mod continues_on;
mod ensure_started;
mod finally;
mod fork_join;
mod into_tuple;
mod into_variant;
mod let_error;
mod let_stopped;
mod let_value;
mod materialize;
mod on;
mod repeat;
mod reschedule;
mod split;
mod starts_on;
mod stopped_as_error;
mod stopped_as_optional;
mod then;
mod unless_stopped;
mod upon_error;
mod upon_stopped;
mod when_all;
mod when_any;
mod write_env;

pub use bulk::*;
pub use continues_on::*;
pub use ensure_started::*;
pub use finally::*;
pub use fork_join::*;
pub use into_tuple::*;
pub use into_variant::*;
pub use let_error::*;
pub use let_stopped::*;
pub use let_value::*;
pub use materialize::*;
pub use on::*;
pub use repeat::*;
pub use reschedule::*;
pub use split::*;
pub use starts_on::*;
pub use stopped_as_error::*;
pub use stopped_as_optional::*;
pub use then::*;
pub use unless_stopped::*;
pub use upon_error::*;
pub use upon_stopped::*;
pub use when_all::*;
pub use when_any::*;
pub use write_env::*;
