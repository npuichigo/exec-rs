//! Finally adaptor - guaranteed cleanup after sender completion.
//!
//! `finally` runs a cleanup sender after the main sender completes,
//! regardless of how it completes (value, error, or stopped).

use crate::traits::{
    CompletionSignatures, OperationState, Receiver, ReceiverOf, ReceiverOfError,
    ReceiverOfStopped, Sender, SenderTo,
};
use std::cell::UnsafeCell;
use std::marker::PhantomData;
use std::pin::Pin;

/// Run a cleanup sender after the main sender completes.
///
/// The cleanup sender runs regardless of how the main sender completes.
/// The result of the main sender is preserved and forwarded after cleanup.
///
/// # Example
///
/// ```rust
/// use exec_rs::factories::just;
/// use exec_rs::adaptors::finally;
/// use exec_rs::consumers::sync_wait;
///
/// // Simple cleanup that just completes
/// let result = sync_wait(finally(just(42), just(()))).unwrap();
/// assert_eq!(result, 42);
/// ```
pub fn finally<S, C>(sender: S, cleanup: C) -> Finally<S, C> {
    Finally::new(sender, cleanup)
}

/// A sender that runs cleanup after the main sender.
#[derive(Debug, Clone)]
pub struct Finally<S, C> {
    sender: S,
    cleanup: C,
}

impl<S, C> Finally<S, C> {
    /// Create a new Finally adaptor.
    pub fn new(sender: S, cleanup: C) -> Self {
        Self { sender, cleanup }
    }
}

/// Completion signatures for Finally.
pub struct FinallyCompletions<V, E>(PhantomData<(V, E)>);

impl<V, E> CompletionSignatures for FinallyCompletions<V, E> {
    type Value = V;
    type Error = E;
    const SENDS_STOPPED: bool = true;
}

impl<S, C, V, E> Sender for Finally<S, C>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
    C: Sender<Completions: CompletionSignatures<Value = ()>>,
{
    type Completions = FinallyCompletions<V, E>;
}

/// Result from the main sender that we need to forward after cleanup.
enum MainResult<V, E> {
    Value(V),
    Error(E),
    Stopped,
}

impl<S, C, R, V, E> SenderTo<R> for Finally<S, C>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>
        + SenderTo<FinallyMainReceiver<V, E, C, R>>,
    C: Sender<Completions: CompletionSignatures<Value = ()>> + Clone,
    R: ReceiverOf<V> + ReceiverOfError<E> + ReceiverOfStopped + Clone,
    V: 'static,
    E: 'static,
{
    type Operation = FinallyOperation<S, C, R, V, E>;

    fn connect(self, receiver: R) -> Self::Operation {
        let state = FinallyState {
            cleanup: Some(self.cleanup),
            receiver: Some(receiver),
            main_result: UnsafeCell::new(None),
            _phantom: PhantomData,
        };

        let main_receiver = FinallyMainReceiver { state };

        FinallyOperation {
            inner: self.sender.connect(main_receiver),
        }
    }
}

/// Shared state for finally operation.
struct FinallyState<V, E, C, R> {
    cleanup: Option<C>,
    receiver: Option<R>,
    main_result: UnsafeCell<Option<MainResult<V, E>>>,
    _phantom: PhantomData<(V, E)>,
}

/// Receiver for the main sender.
pub struct FinallyMainReceiver<V, E, C, R> {
    state: FinallyState<V, E, C, R>,
}

impl<V, E, C, R: Receiver> Receiver for FinallyMainReceiver<V, E, C, R> {
    type Env = R::Env;

    fn get_env(&self) -> &Self::Env {
        self.state.receiver.as_ref().unwrap().get_env()
    }
}

impl<V, E, C, R> ReceiverOf<V> for FinallyMainReceiver<V, E, C, R>
where
    C: Sender<Completions: CompletionSignatures<Value = ()>>
        + SenderTo<FinallyCleanupReceiver<V, E, R>>,
    R: ReceiverOf<V> + ReceiverOfError<E> + ReceiverOfStopped,
    V: 'static,
    E: 'static,
{
    fn set_value(mut self, value: V) {
        // Store the result
        unsafe {
            *self.state.main_result.get() = Some(MainResult::Value(value));
        }

        // Run cleanup
        self.run_cleanup();
    }
}

impl<V, E, C, R> ReceiverOfError<E> for FinallyMainReceiver<V, E, C, R>
where
    C: Sender<Completions: CompletionSignatures<Value = ()>>
        + SenderTo<FinallyCleanupReceiver<V, E, R>>,
    R: ReceiverOf<V> + ReceiverOfError<E> + ReceiverOfStopped,
    V: 'static,
    E: 'static,
{
    fn set_error(mut self, error: E) {
        unsafe {
            *self.state.main_result.get() = Some(MainResult::Error(error));
        }
        self.run_cleanup();
    }
}

impl<V, E, C, R> ReceiverOfStopped for FinallyMainReceiver<V, E, C, R>
where
    C: Sender<Completions: CompletionSignatures<Value = ()>>
        + SenderTo<FinallyCleanupReceiver<V, E, R>>,
    R: ReceiverOf<V> + ReceiverOfError<E> + ReceiverOfStopped,
    V: 'static,
    E: 'static,
{
    fn set_stopped(mut self) {
        unsafe {
            *self.state.main_result.get() = Some(MainResult::Stopped);
        }
        self.run_cleanup();
    }
}

impl<V, E, C, R> FinallyMainReceiver<V, E, C, R>
where
    C: Sender<Completions: CompletionSignatures<Value = ()>>
        + SenderTo<FinallyCleanupReceiver<V, E, R>>,
    R: ReceiverOf<V> + ReceiverOfError<E> + ReceiverOfStopped,
    V: 'static,
    E: 'static,
{
    fn run_cleanup(mut self) {
        let cleanup = self.state.cleanup.take().unwrap();
        let receiver = self.state.receiver.take().unwrap();
        let main_result = unsafe { (*self.state.main_result.get()).take().unwrap() };

        let cleanup_receiver = FinallyCleanupReceiver {
            main_result: Some(main_result),
            receiver: Some(receiver),
            _phantom: PhantomData,
        };

        let mut op = cleanup.connect(cleanup_receiver);
        // SAFETY: We're starting the operation immediately and not moving it
        unsafe {
            Pin::new_unchecked(&mut op).start();
        }
    }
}

/// Receiver for the cleanup sender.
pub struct FinallyCleanupReceiver<V, E, R> {
    main_result: Option<MainResult<V, E>>,
    receiver: Option<R>,
    _phantom: PhantomData<(V, E)>,
}

impl<V, E, R: Receiver> Receiver for FinallyCleanupReceiver<V, E, R> {
    type Env = R::Env;

    fn get_env(&self) -> &Self::Env {
        self.receiver.as_ref().unwrap().get_env()
    }
}

impl<V, E, R> ReceiverOf<()> for FinallyCleanupReceiver<V, E, R>
where
    R: ReceiverOf<V> + ReceiverOfError<E> + ReceiverOfStopped,
{
    fn set_value(mut self, _: ()) {
        // Cleanup succeeded, forward the main result
        let receiver = self.receiver.take().unwrap();
        match self.main_result.take().unwrap() {
            MainResult::Value(v) => receiver.set_value(v),
            MainResult::Error(e) => receiver.set_error(e),
            MainResult::Stopped => receiver.set_stopped(),
        }
    }
}

impl<V, E, CE, R> ReceiverOfError<CE> for FinallyCleanupReceiver<V, E, R>
where
    R: ReceiverOf<V> + ReceiverOfError<E> + ReceiverOfStopped,
{
    fn set_error(mut self, _error: CE) {
        // Cleanup failed, but we still forward the main result
        // (cleanup errors are typically ignored in finally semantics)
        let receiver = self.receiver.take().unwrap();
        match self.main_result.take().unwrap() {
            MainResult::Value(v) => receiver.set_value(v),
            MainResult::Error(e) => receiver.set_error(e),
            MainResult::Stopped => receiver.set_stopped(),
        }
    }
}

impl<V, E, R> ReceiverOfStopped for FinallyCleanupReceiver<V, E, R>
where
    R: ReceiverOf<V> + ReceiverOfError<E> + ReceiverOfStopped,
{
    fn set_stopped(mut self) {
        // Cleanup was stopped, forward main result anyway
        let receiver = self.receiver.take().unwrap();
        match self.main_result.take().unwrap() {
            MainResult::Value(v) => receiver.set_value(v),
            MainResult::Error(e) => receiver.set_error(e),
            MainResult::Stopped => receiver.set_stopped(),
        }
    }
}

/// Operation state for Finally.
pub struct FinallyOperation<S, C, R, V, E>
where
    S: SenderTo<FinallyMainReceiver<V, E, C, R>>,
    R: Receiver,
{
    inner: S::Operation,
}

impl<S, C, R, V, E> OperationState for FinallyOperation<S, C, R, V, E>
where
    S: SenderTo<FinallyMainReceiver<V, E, C, R>>,
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
    use crate::adaptors::then;
    use crate::consumers::sync_wait;
    use crate::factories::just;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[test]
    fn test_finally_basic() {
        let cleaned_up = Arc::new(AtomicBool::new(false));
        let cleaned_up_clone = cleaned_up.clone();

        let result = sync_wait(finally(
            just(42),
            then(just(()), move |_| {
                cleaned_up_clone.store(true, Ordering::SeqCst)
            }),
        ))
        .unwrap();

        assert_eq!(result, 42);
        assert!(cleaned_up.load(Ordering::SeqCst));
    }

    #[test]
    fn test_finally_preserves_value() {
        let result = sync_wait(finally(just("hello"), just(()))).unwrap();
        assert_eq!(result, "hello");
    }
}
