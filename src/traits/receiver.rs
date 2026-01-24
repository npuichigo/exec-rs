//! Receiver traits for handling completion signals.
//!
//! A receiver is a callback object that handles the three completion signals:
//! - `set_value`: Called when the sender completes successfully
//! - `set_error`: Called when the sender completes with an error
//! - `set_stopped`: Called when the sender is cancelled

use crate::traits::env::Environment;

/// The base receiver trait.
///
/// All receivers must provide an environment for queries.
/// This is analogous to `get_env` in C++ P2300.
pub trait Receiver: Sized {
    /// The environment type provided by this receiver.
    type Env: Environment;

    /// Get the receiver's environment.
    ///
    /// The environment provides contextual information like stop tokens
    /// and schedulers to the connected sender.
    fn get_env(&self) -> &Self::Env;
}

/// A receiver that can handle value completion.
///
/// This is the primary completion path - called when the sender
/// produces its result values successfully.
pub trait ReceiverOf<Value>: Receiver {
    /// Handle successful completion with a value.
    ///
    /// This method consumes the receiver - it can only be called once.
    fn set_value(self, value: Value);
}

/// A receiver that can handle error completion.
///
/// Called when the sender encounters an error during execution.
pub trait ReceiverOfError<Error>: Receiver {
    /// Handle error completion.
    ///
    /// This method consumes the receiver - it can only be called once.
    fn set_error(self, error: Error);
}

/// A receiver that can handle stopped completion.
///
/// Called when the operation is cancelled via the stop token mechanism.
pub trait ReceiverOfStopped: Receiver {
    /// Handle stopped/cancelled completion.
    ///
    /// This method consumes the receiver - it can only be called once.
    fn set_stopped(self);
}

/// A complete receiver that handles all three completion signals.
///
/// This is a convenience trait that combines all receiver capabilities.
pub trait FullReceiver<Value, Error>:
    ReceiverOf<Value> + ReceiverOfError<Error> + ReceiverOfStopped
{
}

// Blanket implementation for any type that implements all three
impl<R, V, E> FullReceiver<V, E> for R where
    R: ReceiverOf<V> + ReceiverOfError<E> + ReceiverOfStopped
{
}

/// A receiver adaptor that wraps another receiver.
///
/// This is useful for building receiver chains that transform completions.
pub struct ReceiverAdaptor<R, F> {
    inner: R,
    transform: F,
}

impl<R, F> ReceiverAdaptor<R, F> {
    /// Create a new receiver adaptor.
    pub fn new(inner: R, transform: F) -> Self {
        Self { inner, transform }
    }

    /// Get a reference to the inner receiver.
    pub fn inner(&self) -> &R {
        &self.inner
    }

    /// Unwrap the adaptor, returning the inner receiver.
    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: Receiver, F> Receiver for ReceiverAdaptor<R, F> {
    type Env = R::Env;

    fn get_env(&self) -> &Self::Env {
        self.inner.get_env()
    }
}

/// A void receiver that ignores all completions.
///
/// Useful for fire-and-forget operations or testing.
#[derive(Debug, Clone, Copy, Default)]
pub struct VoidReceiver<Env = ()>(Env);

impl<Env> VoidReceiver<Env> {
    /// Create a new void receiver with the given environment.
    pub fn new(env: Env) -> Self {
        Self(env)
    }
}

impl<Env: Environment> Receiver for VoidReceiver<Env> {
    type Env = Env;

    fn get_env(&self) -> &Self::Env {
        &self.0
    }
}

impl<V, Env: Environment> ReceiverOf<V> for VoidReceiver<Env> {
    fn set_value(self, _value: V) {}
}

impl<E, Env: Environment> ReceiverOfError<E> for VoidReceiver<Env> {
    fn set_error(self, _error: E) {}
}

impl<Env: Environment> ReceiverOfStopped for VoidReceiver<Env> {
    fn set_stopped(self) {}
}
