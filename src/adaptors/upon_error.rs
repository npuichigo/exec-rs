//! UponError adaptor - transforms the error completion.

use crate::traits::{
    CompletionSignatures, OperationState, Receiver, ReceiverOf, ReceiverOfError, ReceiverOfStopped,
    Sender, SenderTo,
};
use std::marker::PhantomData;
use std::pin::Pin;

/// Transform the error of a sender with a function.
///
/// When the upstream sender completes with an error, the function is called
/// with that error, and the result is passed as a value to the downstream receiver.
pub fn upon_error<S, F>(sender: S, f: F) -> UponError<S, F> {
    UponError::new(sender, f)
}

/// A sender that transforms errors with a function.
#[derive(Debug, Clone)]
pub struct UponError<S, F> {
    sender: S,
    f: F,
}

impl<S, F> UponError<S, F> {
    /// Create a new UponError adaptor.
    pub fn new(sender: S, f: F) -> Self {
        Self { sender, f }
    }
}

/// Completion signatures for UponError.
pub struct UponErrorCompletions<V>(PhantomData<V>);

impl<V> CompletionSignatures for UponErrorCompletions<V> {
    type Value = V;
    type Error = std::convert::Infallible;
    const SENDS_STOPPED: bool = true;
}

impl<S, F, U, V, E> Sender for UponError<S, F>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
    F: FnOnce(E) -> U,
{
    type Completions = UponErrorCompletions<U>;
}

impl<S, F, U, R, V, E> SenderTo<R> for UponError<S, F>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>
        + SenderTo<UponErrorReceiver<F, R, V>>,
    F: FnOnce(E) -> U,
    R: ReceiverOf<V> + ReceiverOf<U> + ReceiverOfStopped,
{
    type Operation = UponErrorOperation<S, F, R, V>;

    fn connect(self, receiver: R) -> Self::Operation {
        let inner_receiver = UponErrorReceiver {
            f: Some(self.f),
            receiver: Some(receiver),
            _phantom: PhantomData,
        };
        UponErrorOperation {
            inner: self.sender.connect(inner_receiver),
        }
    }
}

/// Receiver for UponError adaptor.
pub struct UponErrorReceiver<F, R, V> {
    f: Option<F>,
    receiver: Option<R>,
    _phantom: PhantomData<V>,
}

impl<F, R: Receiver, V> Receiver for UponErrorReceiver<F, R, V> {
    type Env = R::Env;

    fn get_env(&self) -> &Self::Env {
        self.receiver.as_ref().unwrap().get_env()
    }
}

impl<F, R, V> ReceiverOf<V> for UponErrorReceiver<F, R, V>
where
    R: ReceiverOf<V>,
{
    fn set_value(mut self, value: V) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_value(value);
        }
    }
}

impl<F, R, E, U, V> ReceiverOfError<E> for UponErrorReceiver<F, R, V>
where
    F: FnOnce(E) -> U,
    R: ReceiverOf<U>,
{
    fn set_error(mut self, error: E) {
        if let (Some(f), Some(receiver)) = (self.f.take(), self.receiver.take()) {
            let result = f(error);
            receiver.set_value(result);
        }
    }
}

impl<F, R, V> ReceiverOfStopped for UponErrorReceiver<F, R, V>
where
    R: ReceiverOfStopped,
{
    fn set_stopped(mut self) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_stopped();
        }
    }
}

/// Operation state for UponError.
pub struct UponErrorOperation<S, F, R, V>
where
    S: SenderTo<UponErrorReceiver<F, R, V>>,
    R: Receiver,
{
    inner: S::Operation,
}

impl<S, F, R, V> OperationState for UponErrorOperation<S, F, R, V>
where
    S: SenderTo<UponErrorReceiver<F, R, V>>,
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
    use crate::factories::just_error;

    #[test]
    fn test_upon_error_basic() {
        // The original value type is () and transformed type must also be ()
        // for sync_wait to work (receiver must implement ReceiverOf for both)
        let result = sync_wait(upon_error(just_error("oops"), |_e| ())).unwrap();
        assert_eq!(result, ());
    }
}
