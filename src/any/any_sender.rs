//! Type-erased sender.

use crate::traits::{
    CompletionSignatures, Environment, OperationState, Receiver, ReceiverOf, ReceiverOfError,
    ReceiverOfStopped, Sender, SenderTo,
};
use std::cell::UnsafeCell;
use std::marker::PhantomData;
use std::pin::Pin;

/// A type-erased sender that produces values of type `V` or errors of type `E`.
///
/// This allows storing heterogeneous senders in collections or returning
/// different sender types from functions with a common signature.
///
/// # Example
///
/// ```rust
/// use exec_rs::any::AnySender;
/// use exec_rs::factories::just;
/// use exec_rs::consumers::sync_wait;
///
/// let sender: AnySender<i32, std::convert::Infallible> = AnySender::new(just(42));
/// let result = sync_wait(sender).unwrap();
/// assert_eq!(result, 42);
/// ```
pub struct AnySender<V, E> {
    inner: Box<dyn ErasedSender<V, E> + Send>,
}

impl<V, E> AnySender<V, E> {
    /// Create a new type-erased sender from any sender with matching completion types.
    pub fn new<S>(sender: S) -> Self
    where
        S: Sender<Completions: CompletionSignatures<Value = V, Error = E>> + Send + 'static,
        V: Send + 'static,
        E: Send + 'static,
        S: for<'a> SenderTo<AnyReceiverRef<'a, V, E>>,
        for<'a> <S as SenderTo<AnyReceiverRef<'a, V, E>>>::Operation: Send,
    {
        AnySender {
            inner: Box::new(SenderWrapper {
                sender: Some(sender),
            }),
        }
    }
}

/// Completion signatures for AnySender.
pub struct AnySenderCompletions<V, E>(PhantomData<(V, E)>);

impl<V, E> CompletionSignatures for AnySenderCompletions<V, E> {
    type Value = V;
    type Error = E;
    const SENDS_STOPPED: bool = true;
}

impl<V, E> Sender for AnySender<V, E> {
    type Completions = AnySenderCompletions<V, E>;
}

impl<V: Send + 'static, E: Send + 'static, R> SenderTo<R> for AnySender<V, E>
where
    R: ReceiverOf<V> + ReceiverOfError<E> + ReceiverOfStopped + Send + 'static,
{
    type Operation = AnySenderOperation<V, E, R>;

    fn connect(mut self, receiver: R) -> Self::Operation {
        AnySenderOperation {
            inner: self.inner.connect_erased(Box::new(ReceiverWrapper {
                receiver: Some(receiver),
                _phantom: PhantomData,
            })),
            started: false,
            _phantom: PhantomData,
        }
    }
}

/// Operation state for AnySender.
pub struct AnySenderOperation<V, E, R> {
    inner: Box<dyn ErasedOperation + Send>,
    started: bool,
    _phantom: PhantomData<(V, E, R)>,
}

impl<V, E, R> AnySenderOperation<V, E, R> {
    #[allow(dead_code)]
    fn new(inner: Box<dyn ErasedOperation + Send>) -> Self {
        Self {
            inner,
            started: false,
            _phantom: PhantomData,
        }
    }
}

impl<V, E, R> OperationState for AnySenderOperation<V, E, R> {
    fn start(self: Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };
        if this.started {
            return;
        }
        this.started = true;
        this.inner.start_erased();
    }
}

// Internal traits for type erasure

trait ErasedSender<V, E>: Send {
    fn connect_erased(
        &mut self,
        receiver: Box<dyn ErasedReceiver<V, E> + Send>,
    ) -> Box<dyn ErasedOperation + Send>;
}

trait ErasedReceiver<V, E>: Send {
    fn set_value_erased(&mut self, value: V);
    fn set_error_erased(&mut self, error: E);
    fn set_stopped_erased(&mut self);
}

trait ErasedOperation: Send {
    fn start_erased(&mut self);
}

// Wrapper types

struct SenderWrapper<S> {
    sender: Option<S>,
}

impl<S, V, E> ErasedSender<V, E> for SenderWrapper<S>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>> + Send + 'static,
    V: Send + 'static,
    E: Send + 'static,
    S: for<'a> SenderTo<AnyReceiverRef<'a, V, E>>,
    for<'a> <S as SenderTo<AnyReceiverRef<'a, V, E>>>::Operation: Send,
{
    fn connect_erased(
        &mut self,
        receiver: Box<dyn ErasedReceiver<V, E> + Send>,
    ) -> Box<dyn ErasedOperation + Send> {
        let sender = self.sender.take().expect("sender already consumed");
        let any_receiver = AnyReceiverRef {
            inner: UnsafeCell::new(Some(receiver)),
            _phantom: PhantomData,
        };
        let operation = sender.connect(any_receiver);
        Box::new(OperationWrapper {
            operation: UnsafeCell::new(Some(operation)),
            started: false,
        })
    }
}

struct ReceiverWrapper<R, V, E> {
    receiver: Option<R>,
    _phantom: PhantomData<(V, E)>,
}

impl<R, V, E> ErasedReceiver<V, E> for ReceiverWrapper<R, V, E>
where
    R: ReceiverOf<V> + ReceiverOfError<E> + ReceiverOfStopped + Send,
    V: Send,
    E: Send,
{
    fn set_value_erased(&mut self, value: V) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_value(value);
        }
    }

    fn set_error_erased(&mut self, error: E) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_error(error);
        }
    }

    fn set_stopped_erased(&mut self) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_stopped();
        }
    }
}

struct OperationWrapper<Op> {
    operation: UnsafeCell<Option<Op>>,
    started: bool,
}

// SAFETY: We only access the cell from start_erased
unsafe impl<Op: Send> Send for OperationWrapper<Op> {}

impl<Op: OperationState + Send> ErasedOperation for OperationWrapper<Op> {
    fn start_erased(&mut self) {
        if self.started {
            return;
        }
        self.started = true;

        let op = unsafe { (*self.operation.get()).as_mut() };
        if let Some(op) = op {
            // SAFETY: We ensure the operation is pinned by not moving it after this
            let pinned = unsafe { Pin::new_unchecked(op) };
            pinned.start();
        }
    }
}

/// A type-erased receiver reference used internally by AnySender.
pub struct AnyReceiverRef<'a, V, E> {
    inner: UnsafeCell<Option<Box<dyn ErasedReceiver<V, E> + Send + 'a>>>,
    _phantom: PhantomData<&'a ()>,
}

// SAFETY: We only access the cell from completion methods
unsafe impl<V: Send, E: Send> Send for AnyReceiverRef<'_, V, E> {}

/// Environment for AnyReceiverRef.
pub struct AnyReceiverEnv;

impl Environment for AnyReceiverEnv {
    type StopToken = crate::stop::NeverStopToken;

    fn get_stop_token(&self) -> Self::StopToken {
        crate::stop::NeverStopToken
    }
}

impl<V, E> Receiver for AnyReceiverRef<'_, V, E> {
    type Env = AnyReceiverEnv;

    fn get_env(&self) -> &Self::Env {
        &AnyReceiverEnv
    }
}

impl<V, E> ReceiverOf<V> for AnyReceiverRef<'_, V, E> {
    fn set_value(self, value: V) {
        let inner = unsafe { (*self.inner.get()).take() };
        if let Some(mut receiver) = inner {
            receiver.set_value_erased(value);
        }
    }
}

impl<V, E> ReceiverOfError<E> for AnyReceiverRef<'_, V, E> {
    fn set_error(self, error: E) {
        let inner = unsafe { (*self.inner.get()).take() };
        if let Some(mut receiver) = inner {
            receiver.set_error_erased(error);
        }
    }
}

impl<V, E> ReceiverOfStopped for AnyReceiverRef<'_, V, E> {
    fn set_stopped(self) {
        let inner = unsafe { (*self.inner.get()).take() };
        if let Some(mut receiver) = inner {
            receiver.set_stopped_erased();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consumers::sync_wait;
    use crate::factories::{just, just_error};

    #[test]
    fn test_any_sender_just() {
        let sender: AnySender<i32, std::convert::Infallible> = AnySender::new(just(42));
        let result = sync_wait(sender).unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn test_any_sender_just_string() {
        let sender: AnySender<String, std::convert::Infallible> =
            AnySender::new(just(String::from("hello")));
        let result = sync_wait(sender).unwrap();
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_any_sender_error() {
        // just_error produces Value = (), so we use that type
        let sender: AnySender<(), &str> = AnySender::new(just_error("oops"));
        let result = sync_wait(sender);
        assert!(result.is_err());
    }

    #[test]
    fn test_any_sender_in_function() {
        fn get_sender(value: i32) -> AnySender<i32, std::convert::Infallible> {
            AnySender::new(just(value))
        }

        let result = sync_wait(get_sender(100)).unwrap();
        assert_eq!(result, 100);
    }

    #[test]
    fn test_any_sender_conditional() {
        fn choose_sender(use_first: bool) -> AnySender<i32, std::convert::Infallible> {
            if use_first {
                AnySender::new(just(1))
            } else {
                AnySender::new(just(2))
            }
        }

        assert_eq!(sync_wait(choose_sender(true)).unwrap(), 1);
        assert_eq!(sync_wait(choose_sender(false)).unwrap(), 2);
    }
}
