//! Completion signal types and signatures.
//!
//! In the P2300 model, senders can complete in three ways:
//! - **Value**: Successful completion with zero or more values
//! - **Error**: Completion with an error
//! - **Stopped**: Completion due to cancellation

use std::marker::PhantomData;

/// The disposition of a completion signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Disposition {
    /// Successful value completion.
    Value,
    /// Error completion.
    Error,
    /// Stopped/cancelled completion.
    Stopped,
}

/// Marker trait for completion signatures.
///
/// This is used to describe what completions a sender can produce.
/// In Rust, we use associated types instead of C++'s completion_signatures template.
pub trait CompletionSignatures {
    /// The value types this sender can complete with.
    type Value;
    /// The error types this sender can complete with.
    type Error;
    /// Whether this sender can complete with stopped.
    const SENDS_STOPPED: bool;
}

/// A completion signatures type that never completes.
#[derive(Debug, Clone, Copy)]
pub struct NeverCompletions;

impl CompletionSignatures for NeverCompletions {
    type Value = std::convert::Infallible;
    type Error = std::convert::Infallible;
    const SENDS_STOPPED: bool = false;
}

/// Completion signatures for a sender that completes with a single value type.
#[derive(Debug, Clone, Copy)]
pub struct ValueCompletion<V, E = std::convert::Infallible>(PhantomData<(V, E)>);

impl<V, E> CompletionSignatures for ValueCompletion<V, E> {
    type Value = V;
    type Error = E;
    const SENDS_STOPPED: bool = false;
}

/// Completion signatures for a sender that can complete with value, error, or stopped.
#[derive(Debug, Clone, Copy)]
pub struct FullCompletions<V, E>(PhantomData<(V, E)>);

impl<V, E> CompletionSignatures for FullCompletions<V, E> {
    type Value = V;
    type Error = E;
    const SENDS_STOPPED: bool = true;
}

/// Completion signatures for a sender that only completes with stopped.
#[derive(Debug, Clone, Copy)]
pub struct StoppedCompletion;

impl CompletionSignatures for StoppedCompletion {
    type Value = ();
    type Error = std::convert::Infallible;
    const SENDS_STOPPED: bool = true;
}

/// Completion signatures for a sender that only completes with an error.
#[derive(Debug, Clone, Copy)]
pub struct ErrorCompletion<E>(PhantomData<E>);

impl<E> CompletionSignatures for ErrorCompletion<E> {
    type Value = ();
    type Error = E;
    const SENDS_STOPPED: bool = false;
}

/// Helper to create value completion signatures.
pub type JustCompletions<V> = ValueCompletion<V, std::convert::Infallible>;

/// Helper to create error-only completion signatures.
pub type JustErrorCompletions<E> = ErrorCompletion<E>;
