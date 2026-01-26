//! IntoTuple adaptor - wraps a value in a single-element tuple.
//!
//! `into_tuple` transforms a sender's value from `V` to `(V,)`.

use crate::traits::{
    CompletionSignatures, OperationState, Receiver, ReceiverOf, ReceiverOfError, ReceiverOfStopped,
    Sender, SenderTo,
};
use std::marker::PhantomData;
use std::pin::Pin;

/// Wrap the value in a single-element tuple.
///
/// When the upstream sender completes with a value, the downstream receiver
/// receives the value wrapped in a tuple `(value,)`.
///
/// # Example
///
/// ```rust
/// use exec_rs::factories::just;
/// use exec_rs::adaptors::into_tuple;
/// use exec_rs::consumers::sync_wait;
///
/// let result = sync_wait(into_tuple(just(42))).unwrap();
/// assert_eq!(result, (42,));
/// ```
pub fn into_tuple<S>(sender: S) -> IntoTuple<S> {
    IntoTuple::new(sender)
}

/// A sender that wraps the value in a tuple.
#[derive(Debug, Clone)]
pub struct IntoTuple<S> {
    sender: S,
}

impl<S> IntoTuple<S> {
    /// Create a new IntoTuple adaptor.
    pub fn new(sender: S) -> Self {
        Self { sender }
    }
}

/// Completion signatures for IntoTuple.
pub struct IntoTupleCompletions<V, E>(PhantomData<(V, E)>);

impl<V, E> CompletionSignatures for IntoTupleCompletions<V, E> {
    type Value = (V,);
    type Error = E;
    const SENDS_STOPPED: bool = true;
}

impl<S, V, E> Sender for IntoTuple<S>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
{
    type Completions = IntoTupleCompletions<V, E>;
}

impl<S, R, V, E> SenderTo<R> for IntoTuple<S>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>
        + SenderTo<IntoTupleReceiver<R, V, E>>,
    R: ReceiverOf<(V,)> + ReceiverOfError<E> + ReceiverOfStopped,
{
    type Operation = IntoTupleOperation<S, R, V, E>;

    fn connect(self, receiver: R) -> Self::Operation {
        let inner_receiver = IntoTupleReceiver {
            receiver: Some(receiver),
            _phantom: PhantomData,
        };
        IntoTupleOperation {
            inner: self.sender.connect(inner_receiver),
        }
    }
}

/// Receiver for IntoTuple adaptor.
pub struct IntoTupleReceiver<R, V, E> {
    receiver: Option<R>,
    _phantom: PhantomData<(V, E)>,
}

impl<R: Receiver, V, E> Receiver for IntoTupleReceiver<R, V, E> {
    type Env = R::Env;

    fn get_env(&self) -> &Self::Env {
        self.receiver.as_ref().unwrap().get_env()
    }
}

impl<R, V, E> ReceiverOf<V> for IntoTupleReceiver<R, V, E>
where
    R: ReceiverOf<(V,)>,
{
    fn set_value(mut self, value: V) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_value((value,));
        }
    }
}

impl<R, V, E> ReceiverOfError<E> for IntoTupleReceiver<R, V, E>
where
    R: ReceiverOfError<E>,
{
    fn set_error(mut self, error: E) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_error(error);
        }
    }
}

impl<R, V, E> ReceiverOfStopped for IntoTupleReceiver<R, V, E>
where
    R: ReceiverOfStopped,
{
    fn set_stopped(mut self) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_stopped();
        }
    }
}

/// Operation state for IntoTuple.
pub struct IntoTupleOperation<S, R, V, E>
where
    S: SenderTo<IntoTupleReceiver<R, V, E>>,
    R: Receiver,
{
    inner: S::Operation,
}

impl<S, R, V, E> OperationState for IntoTupleOperation<S, R, V, E>
where
    S: SenderTo<IntoTupleReceiver<R, V, E>>,
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
    fn test_into_tuple_basic() {
        let result = sync_wait(into_tuple(just(42))).unwrap();
        assert_eq!(result, (42,));
    }

    #[test]
    fn test_into_tuple_string() {
        let result = sync_wait(into_tuple(just("hello"))).unwrap();
        assert_eq!(result, ("hello",));
    }
}
