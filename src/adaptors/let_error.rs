//! LetError adaptor - handle errors with a function that returns a sender.

use crate::traits::{
    CompletionSignatures, OperationState, Receiver, ReceiverOf, ReceiverOfError,
    ReceiverOfStopped, Sender, SenderTo,
};
use std::marker::PhantomData;
use std::pin::{pin, Pin};

/// Handle errors with a function that returns a sender.
///
/// When the upstream sender completes with an error, the function is called
/// with that error to produce a new sender for recovery.
pub fn let_error<S, F>(sender: S, f: F) -> LetError<S, F> {
    LetError::new(sender, f)
}

/// A sender that handles errors with a sender-returning function.
#[derive(Debug, Clone)]
pub struct LetError<S, F> {
    sender: S,
    f: F,
}

impl<S, F> LetError<S, F> {
    /// Create a new LetError adaptor.
    pub fn new(sender: S, f: F) -> Self {
        Self { sender, f }
    }
}

/// Completion signatures for LetError.
pub struct LetErrorCompletions<V, E>(PhantomData<(V, E)>);

impl<V, E> CompletionSignatures for LetErrorCompletions<V, E> {
    type Value = V;
    type Error = E;
    const SENDS_STOPPED: bool = true;
}

impl<S, F, S2, V, E, E2> Sender for LetError<S, F>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
    F: FnOnce(E) -> S2,
    S2: Sender<Completions: CompletionSignatures<Error = E2>>,
{
    type Completions = LetErrorCompletions<V, E2>;
}

impl<S, F, S2, R, V, E> SenderTo<R> for LetError<S, F>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>
        + SenderTo<LetErrorReceiver<F, S2, R, V>>,
    F: FnOnce(E) -> S2,
    S2: Sender + SenderTo<R>,
    R: ReceiverOf<V>
        + ReceiverOf<<S2::Completions as CompletionSignatures>::Value>
        + ReceiverOfError<<S2::Completions as CompletionSignatures>::Error>
        + ReceiverOfStopped
        + Clone,
{
    type Operation = LetErrorOperation<S, F, S2, R, V>;

    fn connect(self, receiver: R) -> Self::Operation {
        let inner_receiver = LetErrorReceiver {
            f: Some(self.f),
            receiver: receiver.clone(),
            _phantom: PhantomData,
        };
        LetErrorOperation {
            inner: self.sender.connect(inner_receiver),
        }
    }
}

/// Receiver for LetError adaptor.
pub struct LetErrorReceiver<F, S2, R, V> {
    f: Option<F>,
    receiver: R,
    _phantom: PhantomData<(S2, V)>,
}

impl<F: Clone, S2, R: Clone, V> Clone for LetErrorReceiver<F, S2, R, V> {
    fn clone(&self) -> Self {
        Self {
            f: self.f.clone(),
            receiver: self.receiver.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<F, S2, R: Receiver, V> Receiver for LetErrorReceiver<F, S2, R, V> {
    type Env = R::Env;

    fn get_env(&self) -> &Self::Env {
        self.receiver.get_env()
    }
}

impl<F, S2, R, V> ReceiverOf<V> for LetErrorReceiver<F, S2, R, V>
where
    R: ReceiverOf<V>,
{
    fn set_value(self, value: V) {
        self.receiver.set_value(value);
    }
}

impl<F, S2, R, E, V> ReceiverOfError<E> for LetErrorReceiver<F, S2, R, V>
where
    F: FnOnce(E) -> S2,
    S2: Sender + SenderTo<R>,
    R: ReceiverOf<<S2::Completions as CompletionSignatures>::Value>
        + ReceiverOfError<<S2::Completions as CompletionSignatures>::Error>
        + ReceiverOfStopped,
{
    fn set_error(mut self, error: E) {
        if let Some(f) = self.f.take() {
            let sender2 = f(error);
            let mut op2 = pin!(sender2.connect(self.receiver));
            op2.as_mut().start();
        }
    }
}

impl<F, S2, R, V> ReceiverOfStopped for LetErrorReceiver<F, S2, R, V>
where
    R: ReceiverOfStopped,
{
    fn set_stopped(self) {
        self.receiver.set_stopped();
    }
}

/// Operation state for LetError.
pub struct LetErrorOperation<S, F, S2, R, V>
where
    S: SenderTo<LetErrorReceiver<F, S2, R, V>>,
    R: Receiver,
{
    inner: S::Operation,
}

impl<S, F, S2, R, V> OperationState for LetErrorOperation<S, F, S2, R, V>
where
    S: SenderTo<LetErrorReceiver<F, S2, R, V>>,
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
    use crate::factories::{just, just_error};

    #[test]
    fn test_let_error_recovery() {
        // just_error produces an error with Value=(), let_error recovers with just(())
        // Both senders must produce the same value type for the receiver
        let sender = let_error(just_error("oops"), |_e: &str| just(()));
        let result = sync_wait(sender).unwrap();
        assert_eq!(result, ());
    }
}
