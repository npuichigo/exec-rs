//! UponStopped adaptor - transforms the stopped completion.

use crate::traits::{
    CompletionSignatures, OperationState, Receiver, ReceiverOf, ReceiverOfError, ReceiverOfStopped,
    Sender, SenderTo,
};
use std::marker::PhantomData;
use std::pin::Pin;

/// Transform stopped into a value with a function.
///
/// When the upstream sender completes with stopped, the function is called
/// and its result is passed as a value to the downstream receiver.
pub fn upon_stopped<S, F>(sender: S, f: F) -> UponStopped<S, F> {
    UponStopped::new(sender, f)
}

/// A sender that transforms stopped with a function.
#[derive(Debug, Clone)]
pub struct UponStopped<S, F> {
    sender: S,
    f: F,
}

impl<S, F> UponStopped<S, F> {
    /// Create a new UponStopped adaptor.
    pub fn new(sender: S, f: F) -> Self {
        Self { sender, f }
    }
}

/// Completion signatures for UponStopped.
pub struct UponStoppedCompletions<V, E>(PhantomData<(V, E)>);

impl<V, E> CompletionSignatures for UponStoppedCompletions<V, E> {
    type Value = V;
    type Error = E;
    const SENDS_STOPPED: bool = false;
}

impl<S, F, U, V, E> Sender for UponStopped<S, F>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
    F: FnOnce() -> U,
{
    type Completions = UponStoppedCompletions<U, E>;
}

impl<S, F, U, R, V, E> SenderTo<R> for UponStopped<S, F>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>
        + SenderTo<UponStoppedReceiver<F, R, V, E>>,
    F: FnOnce() -> U,
    R: ReceiverOf<V> + ReceiverOf<U> + ReceiverOfError<E>,
{
    type Operation = UponStoppedOperation<S, F, R, V, E>;

    fn connect(self, receiver: R) -> Self::Operation {
        let inner_receiver = UponStoppedReceiver {
            f: Some(self.f),
            receiver: Some(receiver),
            _phantom: PhantomData,
        };
        UponStoppedOperation {
            inner: self.sender.connect(inner_receiver),
        }
    }
}

/// Receiver for UponStopped adaptor.
pub struct UponStoppedReceiver<F, R, V, E> {
    f: Option<F>,
    receiver: Option<R>,
    _phantom: PhantomData<(V, E)>,
}

impl<F, R: Receiver, V, E> Receiver for UponStoppedReceiver<F, R, V, E> {
    type Env = R::Env;

    fn get_env(&self) -> &Self::Env {
        self.receiver.as_ref().unwrap().get_env()
    }
}

impl<F, R, V, E> ReceiverOf<V> for UponStoppedReceiver<F, R, V, E>
where
    R: ReceiverOf<V>,
{
    fn set_value(mut self, value: V) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_value(value);
        }
    }
}

impl<F, R, V, E> ReceiverOfError<E> for UponStoppedReceiver<F, R, V, E>
where
    R: ReceiverOfError<E>,
{
    fn set_error(mut self, error: E) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_error(error);
        }
    }
}

impl<F, R, U, V, E> ReceiverOfStopped for UponStoppedReceiver<F, R, V, E>
where
    F: FnOnce() -> U,
    R: ReceiverOf<U>,
{
    fn set_stopped(mut self) {
        if let (Some(f), Some(receiver)) = (self.f.take(), self.receiver.take()) {
            let result = f();
            receiver.set_value(result);
        }
    }
}

/// Operation state for UponStopped.
pub struct UponStoppedOperation<S, F, R, V, E>
where
    S: SenderTo<UponStoppedReceiver<F, R, V, E>>,
    R: Receiver,
{
    inner: S::Operation,
}

impl<S, F, R, V, E> OperationState for UponStoppedOperation<S, F, R, V, E>
where
    S: SenderTo<UponStoppedReceiver<F, R, V, E>>,
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
    use crate::factories::just_stopped;

    #[test]
    fn test_upon_stopped_basic() {
        // The original value type is () and transformed type must also be ()
        // for sync_wait to work (receiver must implement ReceiverOf for both)
        let result = sync_wait(upon_stopped(just_stopped(), || ())).unwrap();
        assert_eq!(result, ());
    }
}
