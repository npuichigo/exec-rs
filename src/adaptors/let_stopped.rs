//! LetStopped adaptor - handle stopped with a function that returns a sender.

use crate::traits::{
    CompletionSignatures, OperationState, Receiver, ReceiverOf, ReceiverOfError,
    ReceiverOfStopped, Sender, SenderTo,
};
use std::marker::PhantomData;
use std::pin::{pin, Pin};

/// Handle stopped with a function that returns a sender.
///
/// When the upstream sender completes with stopped, the function is called
/// to produce a new sender for recovery.
pub fn let_stopped<S, F>(sender: S, f: F) -> LetStopped<S, F> {
    LetStopped::new(sender, f)
}

/// A sender that handles stopped with a sender-returning function.
#[derive(Debug, Clone)]
pub struct LetStopped<S, F> {
    sender: S,
    f: F,
}

impl<S, F> LetStopped<S, F> {
    /// Create a new LetStopped adaptor.
    pub fn new(sender: S, f: F) -> Self {
        Self { sender, f }
    }
}

/// Completion signatures for LetStopped.
pub struct LetStoppedCompletions<V, E>(PhantomData<(V, E)>);

impl<V, E> CompletionSignatures for LetStoppedCompletions<V, E> {
    type Value = V;
    type Error = E;
    const SENDS_STOPPED: bool = false; // Stopped is handled
}

impl<S, F, S2, V, E> Sender for LetStopped<S, F>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
    F: FnOnce() -> S2,
    S2: Sender,
{
    type Completions = LetStoppedCompletions<V, E>;
}

impl<S, F, S2, R, V, E> SenderTo<R> for LetStopped<S, F>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>
        + SenderTo<LetStoppedReceiver<F, S2, R, V, E>>,
    F: FnOnce() -> S2,
    S2: Sender + SenderTo<R>,
    R: ReceiverOf<V>
        + ReceiverOf<<S2::Completions as CompletionSignatures>::Value>
        + ReceiverOfError<E>
        + ReceiverOfError<<S2::Completions as CompletionSignatures>::Error>
        + Clone,
{
    type Operation = LetStoppedOperation<S, F, S2, R, V, E>;

    fn connect(self, receiver: R) -> Self::Operation {
        let inner_receiver = LetStoppedReceiver {
            f: Some(self.f),
            receiver: receiver.clone(),
            _phantom: PhantomData,
        };
        LetStoppedOperation {
            inner: self.sender.connect(inner_receiver),
        }
    }
}

/// Receiver for LetStopped adaptor.
pub struct LetStoppedReceiver<F, S2, R, V, E> {
    f: Option<F>,
    receiver: R,
    _phantom: PhantomData<(S2, V, E)>,
}

impl<F: Clone, S2, R: Clone, V, E> Clone for LetStoppedReceiver<F, S2, R, V, E> {
    fn clone(&self) -> Self {
        Self {
            f: self.f.clone(),
            receiver: self.receiver.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<F, S2, R: Receiver, V, E> Receiver for LetStoppedReceiver<F, S2, R, V, E> {
    type Env = R::Env;

    fn get_env(&self) -> &Self::Env {
        self.receiver.get_env()
    }
}

impl<F, S2, R, V, E> ReceiverOf<V> for LetStoppedReceiver<F, S2, R, V, E>
where
    R: ReceiverOf<V>,
{
    fn set_value(self, value: V) {
        self.receiver.set_value(value);
    }
}

impl<F, S2, R, V, E> ReceiverOfError<E> for LetStoppedReceiver<F, S2, R, V, E>
where
    R: ReceiverOfError<E>,
{
    fn set_error(self, error: E) {
        self.receiver.set_error(error);
    }
}

impl<F, S2, R, V, E> ReceiverOfStopped for LetStoppedReceiver<F, S2, R, V, E>
where
    F: FnOnce() -> S2,
    S2: Sender + SenderTo<R>,
    R: ReceiverOf<<S2::Completions as CompletionSignatures>::Value>
        + ReceiverOfError<<S2::Completions as CompletionSignatures>::Error>,
{
    fn set_stopped(mut self) {
        if let Some(f) = self.f.take() {
            let sender2 = f();
            let mut op2 = pin!(sender2.connect(self.receiver));
            op2.as_mut().start();
        }
    }
}

/// Operation state for LetStopped.
pub struct LetStoppedOperation<S, F, S2, R, V, E>
where
    S: SenderTo<LetStoppedReceiver<F, S2, R, V, E>>,
    R: Receiver,
{
    inner: S::Operation,
}

impl<S, F, S2, R, V, E> OperationState for LetStoppedOperation<S, F, S2, R, V, E>
where
    S: SenderTo<LetStoppedReceiver<F, S2, R, V, E>>,
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
    fn test_let_stopped_recovery() {
        // just_stopped produces stopped, let_stopped recovers with a value
        // Note: just_stopped has Value=(), so recovery sender must also produce ()
        let sender = let_stopped(just_stopped(), || just(()));
        let result = sync_wait(sender).unwrap();
        assert_eq!(result, ());
    }

    #[test]
    fn test_let_stopped_passthrough_value() {
        let sender = let_stopped(just(42), || just(0));
        let result = sync_wait(sender).unwrap();
        assert_eq!(result, 42);
    }
}
