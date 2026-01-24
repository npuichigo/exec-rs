//! UnlessStopped adaptor - checks stop token before proceeding.
//!
//! `unless_stopped` checks whether stop has been requested before
//! continuing with the upstream sender's result.

use crate::stop::StopToken;
use crate::traits::{
    CompletionSignatures, Environment, OperationState, Receiver, ReceiverOf, ReceiverOfError,
    ReceiverOfStopped, Sender, SenderTo,
};
use std::marker::PhantomData;
use std::pin::Pin;

/// Check stop token and complete with stopped if stop was requested.
///
/// This adaptor checks the stop token when a value is received.
/// If stop has been requested, it completes with stopped instead
/// of forwarding the value.
///
/// # Example
///
/// ```rust
/// use exec_rs::factories::just;
/// use exec_rs::adaptors::unless_stopped;
/// use exec_rs::consumers::sync_wait;
///
/// // Normal case - stop not requested, value passes through
/// let result = sync_wait(unless_stopped(just(42))).unwrap();
/// assert_eq!(result, 42);
/// ```
pub fn unless_stopped<S>(sender: S) -> UnlessStopped<S> {
    UnlessStopped::new(sender)
}

/// A sender that checks the stop token.
#[derive(Debug, Clone)]
pub struct UnlessStopped<S> {
    sender: S,
}

impl<S> UnlessStopped<S> {
    /// Create a new UnlessStopped adaptor.
    pub fn new(sender: S) -> Self {
        Self { sender }
    }
}

/// Completion signatures for UnlessStopped.
pub struct UnlessStoppedCompletions<V, E>(PhantomData<(V, E)>);

impl<V, E> CompletionSignatures for UnlessStoppedCompletions<V, E> {
    type Value = V;
    type Error = E;
    const SENDS_STOPPED: bool = true;
}

impl<S, V, E> Sender for UnlessStopped<S>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
{
    type Completions = UnlessStoppedCompletions<V, E>;
}

impl<S, R, V, E> SenderTo<R> for UnlessStopped<S>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>
        + SenderTo<UnlessStoppedReceiver<R, V, E>>,
    R: ReceiverOf<V> + ReceiverOfError<E> + ReceiverOfStopped,
    R::Env: Environment,
    <R::Env as Environment>::StopToken: StopToken,
{
    type Operation = UnlessStoppedOperation<S, R, V, E>;

    fn connect(self, receiver: R) -> Self::Operation {
        let inner_receiver = UnlessStoppedReceiver {
            receiver: Some(receiver),
            _phantom: PhantomData,
        };
        UnlessStoppedOperation {
            inner: self.sender.connect(inner_receiver),
        }
    }
}

/// Receiver for UnlessStopped adaptor.
pub struct UnlessStoppedReceiver<R, V, E> {
    receiver: Option<R>,
    _phantom: PhantomData<(V, E)>,
}

impl<R: Receiver, V, E> Receiver for UnlessStoppedReceiver<R, V, E> {
    type Env = R::Env;

    fn get_env(&self) -> &Self::Env {
        self.receiver.as_ref().unwrap().get_env()
    }
}

impl<R, V, E> ReceiverOf<V> for UnlessStoppedReceiver<R, V, E>
where
    R: ReceiverOf<V> + ReceiverOfStopped,
    R::Env: Environment,
    <R::Env as Environment>::StopToken: StopToken,
{
    fn set_value(mut self, value: V) {
        if let Some(receiver) = self.receiver.take() {
            // Check the stop token
            let stop_token = receiver.get_env().get_stop_token();
            if stop_token.stop_requested() {
                receiver.set_stopped();
            } else {
                receiver.set_value(value);
            }
        }
    }
}

impl<R, V, E> ReceiverOfError<E> for UnlessStoppedReceiver<R, V, E>
where
    R: ReceiverOfError<E>,
{
    fn set_error(mut self, error: E) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_error(error);
        }
    }
}

impl<R, V, E> ReceiverOfStopped for UnlessStoppedReceiver<R, V, E>
where
    R: ReceiverOfStopped,
{
    fn set_stopped(mut self) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_stopped();
        }
    }
}

/// Operation state for UnlessStopped.
pub struct UnlessStoppedOperation<S, R, V, E>
where
    S: SenderTo<UnlessStoppedReceiver<R, V, E>>,
    R: Receiver,
{
    inner: S::Operation,
}

impl<S, R, V, E> OperationState for UnlessStoppedOperation<S, R, V, E>
where
    S: SenderTo<UnlessStoppedReceiver<R, V, E>>,
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
    use crate::factories::just;

    #[test]
    fn test_unless_stopped_passthrough() {
        // When stop is not requested, value passes through
        let result = sync_wait(unless_stopped(just(42))).unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn test_unless_stopped_string() {
        let result = sync_wait(unless_stopped(just("hello"))).unwrap();
        assert_eq!(result, "hello");
    }
}
