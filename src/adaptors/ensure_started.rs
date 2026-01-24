//! Ensure started adaptor - eagerly start a sender.
//!
//! `ensure_started(sender)` starts the sender immediately and returns a new
//! sender that completes with the result when connected. Unlike `split`,
//! this sender is move-only and can only be connected once.

use crate::traits::{
    CompletionSignatures, Environment, OperationState, Receiver, ReceiverOf, ReceiverOfError,
    ReceiverOfStopped, Sender, SenderTo,
};
use std::cell::UnsafeCell;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Eagerly start a sender and return a sender that completes with the result.
///
/// The underlying sender is started immediately when `ensure_started` is called.
/// The returned sender can only be connected once (move-only).
///
/// # Example
///
/// ```rust
/// use exec_rs::adaptors::ensure_started;
/// use exec_rs::factories::just;
/// use exec_rs::consumers::sync_wait;
///
/// // The sender starts immediately
/// let sender = ensure_started(just(42));
///
/// // Later, we can get the result
/// let result = sync_wait(sender).unwrap();
/// assert_eq!(result, 42);
/// ```
pub fn ensure_started<S>(sender: S) -> EnsureStarted<S>
where
    S: Sender + SenderTo<EnsureStartedReceiver<S>> + Send + 'static,
    <S as SenderTo<EnsureStartedReceiver<S>>>::Operation: Send + 'static,
    <S::Completions as CompletionSignatures>::Value: Send,
    <S::Completions as CompletionSignatures>::Error: Send,
{
    let inner = Arc::new(EnsureStartedInner {
        completed: AtomicBool::new(false),
        result: UnsafeCell::new(None),
        waiter: Mutex::new(None),
    });

    // Start the sender immediately
    let receiver = EnsureStartedReceiver {
        inner: inner.clone(),
    };

    let mut op = sender.connect(receiver);

    // SAFETY: We start immediately and the operation needs to complete
    // before the receiver callback fires
    unsafe {
        Pin::new_unchecked(&mut op).start();
    }

    // Store the operation to keep it alive
    // Note: The operation may have already completed synchronously

    EnsureStarted {
        inner,
        _op: Box::new(op),
    }
}

/// A sender that was eagerly started.
pub struct EnsureStarted<S>
where
    S: Sender,
{
    inner: Arc<EnsureStartedInner<S>>,
    _op: Box<dyn Send>, // Keep the operation alive
}

impl<S: Sender> std::fmt::Debug for EnsureStarted<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnsureStarted")
            .field("completed", &self.inner.completed.load(Ordering::Relaxed))
            .finish()
    }
}

struct EnsureStartedInner<S>
where
    S: Sender,
{
    completed: AtomicBool,
    result: UnsafeCell<Option<EnsureStartedResult<S>>>,
    waiter: Mutex<Option<Box<dyn FnOnce(EnsureStartedResult<S>) + Send>>>,
}

// SAFETY: Access is synchronized through atomics and mutex
unsafe impl<S: Sender> Send for EnsureStartedInner<S>
where
    <S::Completions as CompletionSignatures>::Value: Send,
    <S::Completions as CompletionSignatures>::Error: Send,
{
}
unsafe impl<S: Sender> Sync for EnsureStartedInner<S>
where
    <S::Completions as CompletionSignatures>::Value: Send,
    <S::Completions as CompletionSignatures>::Error: Send,
{
}

/// Result stored in the shared state (moved, not cloned).
enum EnsureStartedResult<S: Sender> {
    Value(<S::Completions as CompletionSignatures>::Value),
    Error(<S::Completions as CompletionSignatures>::Error),
    Stopped,
}

/// Completion signatures for ensure_started sender.
pub struct EnsureStartedCompletions<S: Sender> {
    _phantom: PhantomData<S>,
}

impl<S: Sender> CompletionSignatures for EnsureStartedCompletions<S> {
    type Value = <S::Completions as CompletionSignatures>::Value;
    type Error = <S::Completions as CompletionSignatures>::Error;
    const SENDS_STOPPED: bool = S::Completions::SENDS_STOPPED;
}

impl<S: Sender> Sender for EnsureStarted<S> {
    type Completions = EnsureStartedCompletions<S>;
}

impl<S, R> SenderTo<R> for EnsureStarted<S>
where
    S: Sender + Send + 'static,
    <S::Completions as CompletionSignatures>::Value: Send,
    <S::Completions as CompletionSignatures>::Error: Send,
    R: ReceiverOf<<S::Completions as CompletionSignatures>::Value>
        + ReceiverOfError<<S::Completions as CompletionSignatures>::Error>
        + ReceiverOfStopped
        + Send
        + 'static,
{
    type Operation = EnsureStartedOperation<S, R>;

    fn connect(self, receiver: R) -> Self::Operation {
        EnsureStartedOperation {
            inner: self.inner,
            _op: self._op,
            receiver: Arc::new(ReceiverSlot {
                receiver: UnsafeCell::new(Some(receiver)),
                notified: AtomicBool::new(false),
            }),
        }
    }
}

/// Slot holding the receiver - wrapped in Arc for safe sharing with callback.
struct ReceiverSlot<R> {
    receiver: UnsafeCell<Option<R>>,
    notified: AtomicBool,
}

// SAFETY: Access is synchronized through atomic flag
unsafe impl<R: Send> Send for ReceiverSlot<R> {}
unsafe impl<R: Send> Sync for ReceiverSlot<R> {}

/// Operation state for ensure_started sender.
pub struct EnsureStartedOperation<S, R>
where
    S: Sender,
{
    inner: Arc<EnsureStartedInner<S>>,
    _op: Box<dyn Send>,
    receiver: Arc<ReceiverSlot<R>>,
}

// SAFETY: Fields are Send
unsafe impl<S: Sender + Send, R: Send> Send for EnsureStartedOperation<S, R>
where
    <S::Completions as CompletionSignatures>::Value: Send,
    <S::Completions as CompletionSignatures>::Error: Send,
{
}

/// Environment for ensure_started receiver.
pub struct EnsureStartedEnv;

impl Environment for EnsureStartedEnv {
    type StopToken = crate::stop::NeverStopToken;

    fn get_stop_token(&self) -> Self::StopToken {
        crate::stop::NeverStopToken
    }
}

/// Receiver for the underlying sender in ensure_started.
pub struct EnsureStartedReceiver<S: Sender> {
    inner: Arc<EnsureStartedInner<S>>,
}

impl<S: Sender> Receiver for EnsureStartedReceiver<S> {
    type Env = EnsureStartedEnv;

    fn get_env(&self) -> &Self::Env {
        &EnsureStartedEnv
    }
}

impl<S> ReceiverOf<<S::Completions as CompletionSignatures>::Value> for EnsureStartedReceiver<S>
where
    S: Sender + Send + 'static,
    <S::Completions as CompletionSignatures>::Value: Send,
    <S::Completions as CompletionSignatures>::Error: Send,
{
    fn set_value(self, value: <S::Completions as CompletionSignatures>::Value) {
        self.complete(EnsureStartedResult::Value(value));
    }
}

impl<S> ReceiverOfError<<S::Completions as CompletionSignatures>::Error> for EnsureStartedReceiver<S>
where
    S: Sender + Send + 'static,
    <S::Completions as CompletionSignatures>::Value: Send,
    <S::Completions as CompletionSignatures>::Error: Send,
{
    fn set_error(self, error: <S::Completions as CompletionSignatures>::Error) {
        self.complete(EnsureStartedResult::Error(error));
    }
}

impl<S> ReceiverOfStopped for EnsureStartedReceiver<S>
where
    S: Sender + Send + 'static,
    <S::Completions as CompletionSignatures>::Value: Send,
    <S::Completions as CompletionSignatures>::Error: Send,
{
    fn set_stopped(self) {
        self.complete(EnsureStartedResult::Stopped);
    }
}

impl<S> EnsureStartedReceiver<S>
where
    S: Sender + Send + 'static,
    <S::Completions as CompletionSignatures>::Value: Send,
    <S::Completions as CompletionSignatures>::Error: Send,
{
    fn complete(self, result: EnsureStartedResult<S>) {
        // Check if there's a waiter
        let waiter = {
            let mut guard = self.inner.waiter.lock().unwrap();
            guard.take()
        };

        if let Some(waiter) = waiter {
            // Waiter is already registered - notify immediately
            waiter(result);
        } else {
            // Store the result for later retrieval
            unsafe {
                *self.inner.result.get() = Some(result);
            }
            self.inner.completed.store(true, Ordering::Release);
        }
    }
}

fn notify_receiver_ensure_started<S, R>(slot: &ReceiverSlot<R>, result: EnsureStartedResult<S>)
where
    S: Sender,
    R: ReceiverOf<<S::Completions as CompletionSignatures>::Value>
        + ReceiverOfError<<S::Completions as CompletionSignatures>::Error>
        + ReceiverOfStopped,
{
    if !slot.notified.swap(true, Ordering::AcqRel) {
        let receiver = unsafe { (*slot.receiver.get()).take() };
        if let Some(receiver) = receiver {
            match result {
                EnsureStartedResult::Value(v) => receiver.set_value(v),
                EnsureStartedResult::Error(e) => receiver.set_error(e),
                EnsureStartedResult::Stopped => receiver.set_stopped(),
            }
        }
    }
}

impl<S, R> OperationState for EnsureStartedOperation<S, R>
where
    S: Sender + Send + 'static,
    <S::Completions as CompletionSignatures>::Value: Send,
    <S::Completions as CompletionSignatures>::Error: Send,
    R: ReceiverOf<<S::Completions as CompletionSignatures>::Value>
        + ReceiverOfError<<S::Completions as CompletionSignatures>::Error>
        + ReceiverOfStopped
        + Send
        + 'static,
{
    fn start(self: Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };

        // Check if already completed
        if this.inner.completed.load(Ordering::Acquire) {
            let result = unsafe { (*this.inner.result.get()).take() };
            if let Some(result) = result {
                notify_receiver_ensure_started::<S, R>(&this.receiver, result);
            }
            return;
        }

        // Register as a waiter using Arc<ReceiverSlot>
        let receiver_slot = this.receiver.clone();

        let callback: Box<dyn FnOnce(EnsureStartedResult<S>) + Send> = Box::new(move |result| {
            notify_receiver_ensure_started::<S, R>(&receiver_slot, result);
        });

        {
            let mut waiter = this.inner.waiter.lock().unwrap();

            // Double-check completion while holding the lock
            if this.inner.completed.load(Ordering::Acquire) {
                drop(waiter);
                let result = unsafe { (*this.inner.result.get()).take() };
                if let Some(result) = result {
                    notify_receiver_ensure_started::<S, R>(&this.receiver, result);
                }
                return;
            }

            *waiter = Some(callback);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adaptors::then;
    use crate::consumers::sync_wait;
    use crate::factories::{just, just_error};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn test_ensure_started_basic() {
        let sender = ensure_started(just(42));
        let result = sync_wait(sender).unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn test_ensure_started_runs_eagerly() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        // Create the ensure_started sender - this should run the work immediately
        let sender = ensure_started(then(just(()), move |_| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
            42
        }));

        // The work should already be done
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        // But we can still get the result
        let result = sync_wait(sender).unwrap();
        assert_eq!(result, 42);

        // Work was only done once
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_ensure_started_error() {
        let sender = ensure_started(just_error("error"));
        let result = sync_wait(sender);
        assert!(result.is_err());
    }
}
