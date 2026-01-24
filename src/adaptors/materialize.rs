//! Materialize adaptor - converts completions to a completion variant.
//!
//! `materialize` transforms all completion signals (value, error, stopped)
//! into a single value completion containing a `Completion<V, E>` enum.

use crate::traits::{
    CompletionSignatures, OperationState, Receiver, ReceiverOf, ReceiverOfError,
    ReceiverOfStopped, Sender, SenderTo,
};
use std::marker::PhantomData;
use std::pin::Pin;

/// Represents a materialized completion signal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Completion<V, E> {
    /// The sender completed with a value.
    Value(V),
    /// The sender completed with an error.
    Error(E),
    /// The sender was stopped.
    Stopped,
}

/// Materialize all completions into a single `Completion<V, E>` value.
///
/// This adaptor transforms a sender so that all completion signals become
/// a value completion containing a `Completion` enum.
///
/// # Example
///
/// ```rust
/// use exec_rs::factories::{just, just_stopped};
/// use exec_rs::adaptors::{materialize, Completion};
/// use exec_rs::consumers::sync_wait;
///
/// // Value completion becomes Completion::Value
/// let result = sync_wait(materialize(just(42))).unwrap();
/// assert!(matches!(result, Completion::Value(42)));
///
/// // Stopped completion becomes Completion::Stopped
/// let result = sync_wait(materialize(just_stopped())).unwrap();
/// assert!(matches!(result, Completion::Stopped));
/// ```
pub fn materialize<S>(sender: S) -> Materialize<S> {
    Materialize::new(sender)
}

/// A sender that materializes all completions.
#[derive(Debug, Clone)]
pub struct Materialize<S> {
    sender: S,
}

impl<S> Materialize<S> {
    /// Create a new Materialize adaptor.
    pub fn new(sender: S) -> Self {
        Self { sender }
    }
}

/// Completion signatures for Materialize.
/// The value type becomes `Completion<V, E>` and there's no error or stopped.
pub struct MaterializeCompletions<V, E>(PhantomData<(V, E)>);

impl<V, E> CompletionSignatures for MaterializeCompletions<V, E> {
    type Value = Completion<V, E>;
    type Error = std::convert::Infallible;
    const SENDS_STOPPED: bool = false;
}

impl<S, V, E> Sender for Materialize<S>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
{
    type Completions = MaterializeCompletions<V, E>;
}

impl<S, R, V, E> SenderTo<R> for Materialize<S>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>
        + SenderTo<MaterializeReceiver<R, V, E>>,
    R: ReceiverOf<Completion<V, E>>,
{
    type Operation = MaterializeOperation<S, R, V, E>;

    fn connect(self, receiver: R) -> Self::Operation {
        let inner_receiver = MaterializeReceiver {
            receiver: Some(receiver),
            _phantom: PhantomData,
        };
        MaterializeOperation {
            inner: self.sender.connect(inner_receiver),
        }
    }
}

/// Receiver for Materialize adaptor.
pub struct MaterializeReceiver<R, V, E> {
    receiver: Option<R>,
    _phantom: PhantomData<(V, E)>,
}

impl<R: Receiver, V, E> Receiver for MaterializeReceiver<R, V, E> {
    type Env = R::Env;

    fn get_env(&self) -> &Self::Env {
        self.receiver.as_ref().unwrap().get_env()
    }
}

impl<R, V, E> ReceiverOf<V> for MaterializeReceiver<R, V, E>
where
    R: ReceiverOf<Completion<V, E>>,
{
    fn set_value(mut self, value: V) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_value(Completion::Value(value));
        }
    }
}

impl<R, V, E> ReceiverOfError<E> for MaterializeReceiver<R, V, E>
where
    R: ReceiverOf<Completion<V, E>>,
{
    fn set_error(mut self, error: E) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_value(Completion::Error(error));
        }
    }
}

impl<R, V, E> ReceiverOfStopped for MaterializeReceiver<R, V, E>
where
    R: ReceiverOf<Completion<V, E>>,
{
    fn set_stopped(mut self) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_value(Completion::Stopped);
        }
    }
}

/// Operation state for Materialize.
pub struct MaterializeOperation<S, R, V, E>
where
    S: SenderTo<MaterializeReceiver<R, V, E>>,
    R: Receiver,
{
    inner: S::Operation,
}

impl<S, R, V, E> OperationState for MaterializeOperation<S, R, V, E>
where
    S: SenderTo<MaterializeReceiver<R, V, E>>,
    R: Receiver,
{
    fn start(self: Pin<&mut Self>) {
        unsafe {
            let this = self.get_unchecked_mut();
            Pin::new_unchecked(&mut this.inner).start();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consumers::sync_wait;
    use crate::factories::{just, just_stopped};

    #[test]
    fn test_materialize_value() {
        let result = sync_wait(materialize(just(42))).unwrap();
        assert_eq!(result, Completion::Value(42));
    }

    #[test]
    fn test_materialize_stopped() {
        let result = sync_wait(materialize(just_stopped())).unwrap();
        assert_eq!(result, Completion::Stopped);
    }

    #[test]
    fn test_materialize_string() {
        let result = sync_wait(materialize(just("hello"))).unwrap();
        assert_eq!(result, Completion::Value("hello"));
    }
}
