//! StoppedAsOptional adaptor - converts stopped to None.
//!
//! `stopped_as_optional` transforms a sender so that if it completes with
//! stopped, it instead completes with `None`. If it completes with a value,
//! the value is wrapped in `Some`.

use crate::traits::{
    CompletionSignatures, OperationState, Receiver, ReceiverOf, ReceiverOfError, ReceiverOfStopped,
    Sender, SenderTo,
};
use std::marker::PhantomData;
use std::pin::Pin;

/// Transform stopped into `None` and values into `Some(value)`.
///
/// When the upstream sender completes with stopped, the downstream receiver
/// receives `None`. When it completes with a value, the downstream receives
/// `Some(value)`.
///
/// # Example
///
/// ```rust
/// use exec_rs::factories::{just, just_stopped};
/// use exec_rs::adaptors::stopped_as_optional;
/// use exec_rs::consumers::sync_wait;
///
/// // Value becomes Some(value)
/// let result = sync_wait(stopped_as_optional(just(42))).unwrap();
/// assert_eq!(result, Some(42));
///
/// // Stopped becomes None (just_stopped has Value = ())
/// let result = sync_wait(stopped_as_optional(just_stopped())).unwrap();
/// assert_eq!(result, None);
/// ```
pub fn stopped_as_optional<S>(sender: S) -> StoppedAsOptional<S> {
    StoppedAsOptional::new(sender)
}

/// A sender that transforms stopped to None.
#[derive(Debug, Clone)]
pub struct StoppedAsOptional<S> {
    sender: S,
}

impl<S> StoppedAsOptional<S> {
    /// Create a new StoppedAsOptional adaptor.
    pub fn new(sender: S) -> Self {
        Self { sender }
    }
}

/// Completion signatures for StoppedAsOptional.
pub struct StoppedAsOptionalCompletions<V, E>(PhantomData<(V, E)>);

impl<V, E> CompletionSignatures for StoppedAsOptionalCompletions<V, E> {
    type Value = Option<V>;
    type Error = E;
    const SENDS_STOPPED: bool = false;
}

impl<S, V, E> Sender for StoppedAsOptional<S>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
{
    type Completions = StoppedAsOptionalCompletions<V, E>;
}

impl<S, R, V, E> SenderTo<R> for StoppedAsOptional<S>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>
        + SenderTo<StoppedAsOptionalReceiver<R, V, E>>,
    R: ReceiverOf<Option<V>> + ReceiverOfError<E>,
{
    type Operation = StoppedAsOptionalOperation<S, R, V, E>;

    fn connect(self, receiver: R) -> Self::Operation {
        let inner_receiver = StoppedAsOptionalReceiver {
            receiver: Some(receiver),
            _phantom: PhantomData,
        };
        StoppedAsOptionalOperation {
            inner: self.sender.connect(inner_receiver),
        }
    }
}

/// Receiver for StoppedAsOptional adaptor.
pub struct StoppedAsOptionalReceiver<R, V, E> {
    receiver: Option<R>,
    _phantom: PhantomData<(V, E)>,
}

impl<R: Receiver, V, E> Receiver for StoppedAsOptionalReceiver<R, V, E> {
    type Env = R::Env;

    fn get_env(&self) -> &Self::Env {
        self.receiver.as_ref().unwrap().get_env()
    }
}

impl<R, V, E> ReceiverOf<V> for StoppedAsOptionalReceiver<R, V, E>
where
    R: ReceiverOf<Option<V>>,
{
    fn set_value(mut self, value: V) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_value(Some(value));
        }
    }
}

impl<R, V, E> ReceiverOfError<E> for StoppedAsOptionalReceiver<R, V, E>
where
    R: ReceiverOfError<E>,
{
    fn set_error(mut self, error: E) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_error(error);
        }
    }
}

impl<R, V, E> ReceiverOfStopped for StoppedAsOptionalReceiver<R, V, E>
where
    R: ReceiverOf<Option<V>>,
{
    fn set_stopped(mut self) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_value(None);
        }
    }
}

/// Operation state for StoppedAsOptional.
pub struct StoppedAsOptionalOperation<S, R, V, E>
where
    S: SenderTo<StoppedAsOptionalReceiver<R, V, E>>,
    R: Receiver,
{
    inner: S::Operation,
}

impl<S, R, V, E> OperationState for StoppedAsOptionalOperation<S, R, V, E>
where
    S: SenderTo<StoppedAsOptionalReceiver<R, V, E>>,
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
    fn test_stopped_as_optional_value() {
        let result = sync_wait(stopped_as_optional(just(42))).unwrap();
        assert_eq!(result, Some(42));
    }

    #[test]
    fn test_stopped_as_optional_stopped() {
        // just_stopped() has Value = (), so result is Option<()>
        let result = sync_wait(stopped_as_optional(just_stopped())).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_stopped_as_optional_string() {
        let result = sync_wait(stopped_as_optional(just("hello"))).unwrap();
        assert_eq!(result, Some("hello"));
    }
}
