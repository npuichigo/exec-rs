//! LetValue adaptor - chain with a function that returns a sender.

use crate::traits::{
    CompletionSignatures, OperationState, Receiver, ReceiverOf, ReceiverOfError, ReceiverOfStopped,
    Sender, SenderTo,
};
use std::marker::PhantomData;
use std::pin::{pin, Pin};

/// Chain with a function that returns a sender.
///
/// When the upstream sender completes with a value, the function is called
/// with that value to produce a new sender. That sender is then connected
/// and started, with its results passed to the downstream receiver.
///
/// # Example
///
/// ```rust
/// use exec_rs::factories::just;
/// use exec_rs::adaptors::let_value;
/// use exec_rs::consumers::sync_wait;
///
/// let result = sync_wait(let_value(just(21), |x| just(x * 2))).unwrap();
/// assert_eq!(result, 42);
/// ```
pub fn let_value<S, F>(sender: S, f: F) -> LetValue<S, F> {
    LetValue::new(sender, f)
}

/// A sender that chains with a function returning a sender.
#[derive(Debug, Clone)]
pub struct LetValue<S, F> {
    sender: S,
    f: F,
}

impl<S, F> LetValue<S, F> {
    /// Create a new LetValue adaptor.
    pub fn new(sender: S, f: F) -> Self {
        Self { sender, f }
    }
}

/// Completion signatures for LetValue.
pub struct LetValueCompletions<V, E>(PhantomData<(V, E)>);

impl<V, E> CompletionSignatures for LetValueCompletions<V, E> {
    type Value = V;
    type Error = E;
    const SENDS_STOPPED: bool = true;
}

impl<S, F, S2, V, E> Sender for LetValue<S, F>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
    F: FnOnce(V) -> S2,
    S2: Sender,
{
    type Completions = LetValueCompletions<<S2::Completions as CompletionSignatures>::Value, E>;
}

impl<S, F, S2, R, V, E> SenderTo<R> for LetValue<S, F>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>
        + SenderTo<LetValueReceiver<F, S2, R, E>>,
    F: FnOnce(V) -> S2,
    S2: Sender + SenderTo<R>,
    R: ReceiverOf<<S2::Completions as CompletionSignatures>::Value>
        + ReceiverOfError<E>
        + ReceiverOfError<<S2::Completions as CompletionSignatures>::Error>
        + ReceiverOfStopped,
{
    type Operation = LetValueOperation<S, F, S2, R, E>;

    fn connect(self, receiver: R) -> Self::Operation {
        let inner_receiver = LetValueReceiver {
            f: Some(self.f),
            receiver,
            _phantom: PhantomData,
        };
        LetValueOperation {
            inner: self.sender.connect(inner_receiver),
        }
    }
}

/// Receiver for the first sender in LetValue.
pub struct LetValueReceiver<F, S2, R, E> {
    f: Option<F>,
    receiver: R,
    _phantom: PhantomData<(S2, E)>,
}

impl<F: Clone, S2, R: Clone, E> Clone for LetValueReceiver<F, S2, R, E> {
    fn clone(&self) -> Self {
        Self {
            f: self.f.clone(),
            receiver: self.receiver.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<F, S2, R: Receiver, E> Receiver for LetValueReceiver<F, S2, R, E> {
    type Env = R::Env;

    fn get_env(&self) -> &Self::Env {
        self.receiver.get_env()
    }
}

impl<F, S2, R, V, E> ReceiverOf<V> for LetValueReceiver<F, S2, R, E>
where
    F: FnOnce(V) -> S2,
    S2: Sender + SenderTo<R>,
    R: ReceiverOf<<S2::Completions as CompletionSignatures>::Value>
        + ReceiverOfError<<S2::Completions as CompletionSignatures>::Error>
        + ReceiverOfStopped,
{
    fn set_value(mut self, value: V) {
        if let Some(f) = self.f.take() {
            let sender2 = f(value);
            let mut op2 = pin!(sender2.connect(self.receiver));
            // Start the second operation immediately
            op2.as_mut().start();
        }
    }
}

impl<F, S2, R, E> ReceiverOfError<E> for LetValueReceiver<F, S2, R, E>
where
    R: ReceiverOfError<E>,
{
    fn set_error(self, error: E) {
        self.receiver.set_error(error);
    }
}

impl<F, S2, R, E> ReceiverOfStopped for LetValueReceiver<F, S2, R, E>
where
    R: ReceiverOfStopped,
{
    fn set_stopped(self) {
        self.receiver.set_stopped();
    }
}

/// Operation state for LetValue.
pub struct LetValueOperation<S, F, S2, R, E>
where
    S: SenderTo<LetValueReceiver<F, S2, R, E>>,
    R: Receiver,
{
    inner: S::Operation,
}

impl<S, F, S2, R, E> OperationState for LetValueOperation<S, F, S2, R, E>
where
    S: SenderTo<LetValueReceiver<F, S2, R, E>>,
    R: Receiver,
{
    fn start(self: Pin<&mut Self>) {
        // SAFETY: We carefully manage state transitions
        let this = unsafe { self.get_unchecked_mut() };
        // SAFETY: The operation is part of our pinned structure
        unsafe { Pin::new_unchecked(&mut this.inner).start() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consumers::sync_wait;
    use crate::factories::just;

    #[test]
    fn test_let_value_basic() {
        let result = sync_wait(let_value(just(21), |x| just(x * 2))).unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn test_let_value_type_change() {
        let result = sync_wait(let_value(just(42), |x| just(format!("value: {}", x)))).unwrap();
        assert_eq!(result, "value: 42");
    }
}
