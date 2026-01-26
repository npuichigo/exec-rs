//! Then adaptor - transforms the value completion.

use crate::traits::{
    CompletionSignatures, OperationState, Receiver, ReceiverOf, ReceiverOfError, ReceiverOfStopped,
    Sender, SenderTo,
};
use std::marker::PhantomData;
use std::pin::Pin;

/// Transform the value of a sender with a function.
///
/// When the upstream sender completes with a value, the function is called
/// with that value, and the result is passed to the downstream receiver.
///
/// # Example
///
/// ```rust
/// use exec_rs::factories::just;
/// use exec_rs::adaptors::then;
/// use exec_rs::consumers::sync_wait;
///
/// let result = sync_wait(then(just(21), |x| x * 2)).unwrap();
/// assert_eq!(result, 42);
/// ```
pub fn then<S, F>(sender: S, f: F) -> Then<S, F> {
    Then::new(sender, f)
}

/// A sender that transforms values with a function.
#[derive(Debug, Clone)]
pub struct Then<S, F> {
    sender: S,
    f: F,
}

impl<S, F> Then<S, F> {
    /// Create a new Then adaptor.
    pub fn new(sender: S, f: F) -> Self {
        Self { sender, f }
    }
}

/// Completion signatures for Then.
pub struct ThenCompletions<V, E>(PhantomData<(V, E)>);

impl<V, E> CompletionSignatures for ThenCompletions<V, E> {
    type Value = V;
    type Error = E;
    const SENDS_STOPPED: bool = true;
}

/// Helper trait to get completions from a sender.
pub trait SenderCompletions: Sender {
    /// The value type.
    type Value;
    /// The error type.
    type Error;
}

impl<S: Sender> SenderCompletions for S {
    type Value = <S::Completions as CompletionSignatures>::Value;
    type Error = <S::Completions as CompletionSignatures>::Error;
}

impl<S, F, U> Sender for Then<S, F>
where
    S: Sender,
    F: FnOnce(<S as SenderCompletions>::Value) -> U,
{
    type Completions = ThenCompletions<U, <S as SenderCompletions>::Error>;
}

impl<S, F, U, R, V, E> SenderTo<R> for Then<S, F>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>
        + SenderTo<ThenReceiver<F, R, E>>,
    F: FnOnce(V) -> U,
    R: ReceiverOf<U> + ReceiverOfError<E> + ReceiverOfStopped,
{
    type Operation = ThenOperation<S, F, R, E>;

    fn connect(self, receiver: R) -> Self::Operation {
        let inner_receiver = ThenReceiver {
            f: Some(self.f),
            receiver: Some(receiver),
            _phantom: PhantomData,
        };
        ThenOperation {
            inner: self.sender.connect(inner_receiver),
        }
    }
}

/// Receiver for Then adaptor.
pub struct ThenReceiver<F, R, E> {
    f: Option<F>,
    receiver: Option<R>,
    _phantom: PhantomData<E>,
}

impl<F, R: Receiver, E> Receiver for ThenReceiver<F, R, E> {
    type Env = R::Env;

    fn get_env(&self) -> &Self::Env {
        self.receiver.as_ref().unwrap().get_env()
    }
}

impl<F, R, V, U, E> ReceiverOf<V> for ThenReceiver<F, R, E>
where
    F: FnOnce(V) -> U,
    R: ReceiverOf<U>,
{
    fn set_value(mut self, value: V) {
        if let (Some(f), Some(receiver)) = (self.f.take(), self.receiver.take()) {
            let result = f(value);
            receiver.set_value(result);
        }
    }
}

impl<F, R, E> ReceiverOfError<E> for ThenReceiver<F, R, E>
where
    R: ReceiverOfError<E>,
{
    fn set_error(mut self, error: E) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_error(error);
        }
    }
}

impl<F, R, E> ReceiverOfStopped for ThenReceiver<F, R, E>
where
    R: ReceiverOfStopped,
{
    fn set_stopped(mut self) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_stopped();
        }
    }
}

/// Operation state for Then.
pub struct ThenOperation<S, F, R, E>
where
    S: SenderTo<ThenReceiver<F, R, E>>,
    R: Receiver,
{
    inner: S::Operation,
}

impl<S, F, R, E> OperationState for ThenOperation<S, F, R, E>
where
    S: SenderTo<ThenReceiver<F, R, E>>,
    R: Receiver,
{
    fn start(self: Pin<&mut Self>) {
        // SAFETY: We only access inner which can be pinned
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
    fn test_then_basic() {
        let result = sync_wait(then(just(21), |x| x * 2)).unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn test_then_type_change() {
        let result = sync_wait(then(just(42), |x| format!("value: {}", x))).unwrap();
        assert_eq!(result, "value: 42");
    }

    #[test]
    fn test_then_chained() {
        let result = sync_wait(then(then(just(10), |x| x + 5), |x| x * 2)).unwrap();
        assert_eq!(result, 30);
    }

    #[test]
    fn test_then_with_sender_ext() {
        use crate::traits::SenderExt;

        let result = sync_wait(just(21).then(|x| x * 2)).unwrap();
        assert_eq!(result, 42);
    }
}
