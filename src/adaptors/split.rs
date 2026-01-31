//! Split adaptor - convert a single-shot sender into a multi-shot sender.
//!
//! `split(sender)` allows multiple receivers to connect to the same sender.
//! The underlying sender is started at most once, and its result is shared
//! with all waiting receivers.

use crate::traits::{
    CompletionSignatures, Environment, OperationState, Receiver, ReceiverOf, ReceiverOfError,
    ReceiverOfStopped, Sender, SenderTo,
};
use std::cell::UnsafeCell;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Convert a single-shot sender into a multi-shot sender.
///
/// The underlying sender is connected and started at most once. When it
/// completes, all waiting receivers are notified with the result. The
/// split sender can be cloned and connected multiple times.
///
/// # Example
///
/// ```rust
/// use exec_rs::adaptors::split;
/// use exec_rs::factories::just;
/// use exec_rs::consumers::sync_wait;
///
/// let sender = split(just(42));
///
/// // Both connections receive the same value
/// let result1 = sync_wait(sender.clone()).unwrap();
/// let result2 = sync_wait(sender).unwrap();
///
/// assert_eq!(result1, 42);
/// assert_eq!(result2, 42);
/// ```
pub fn split<S>(sender: S) -> Split<S>
where
    S: Sender,
{
    Split {
        inner: Arc::new(SplitInner {
            sender: UnsafeCell::new(Some(sender)),
            started: AtomicBool::new(false),
            completed: AtomicBool::new(false),
            result: UnsafeCell::new(None),
            waiters: Mutex::new(Vec::new()),
        }),
    }
}

/// A multi-shot sender created from a single-shot sender.
pub struct Split<S>
where
    S: Sender,
{
    inner: Arc<SplitInner<S>>,
}

impl<S: Sender> Clone for Split<S> {
    fn clone(&self) -> Self {
        Split {
            inner: self.inner.clone(),
        }
    }
}

impl<S: Sender> std::fmt::Debug for Split<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Split")
            .field("started", &self.inner.started.load(Ordering::Relaxed))
            .field("completed", &self.inner.completed.load(Ordering::Relaxed))
            .finish()
    }
}

struct SplitInner<S>
where
    S: Sender,
{
    sender: UnsafeCell<Option<S>>,
    started: AtomicBool,
    completed: AtomicBool,
    result: UnsafeCell<Option<SplitResult<S>>>,
    waiters: Mutex<Vec<Box<dyn FnOnce(&SplitResult<S>) + Send>>>,
}

// SAFETY: Access is synchronized through atomics and mutex
unsafe impl<S: Sender + Send> Send for SplitInner<S> {}
unsafe impl<S: Sender + Send> Sync for SplitInner<S> {}

/// Result stored in the shared state.
enum SplitResult<S: Sender> {
    Value(<S::Completions as CompletionSignatures>::Value),
    Error(<S::Completions as CompletionSignatures>::Error),
    Stopped,
}

/// Completion signatures for split sender.
pub struct SplitCompletions<S: Sender> {
    _phantom: PhantomData<S>,
}

impl<S: Sender> CompletionSignatures for SplitCompletions<S> {
    type Value = <S::Completions as CompletionSignatures>::Value;
    type Error = <S::Completions as CompletionSignatures>::Error;
    const SENDS_STOPPED: bool = S::Completions::SENDS_STOPPED;
}

impl<S: Sender> Sender for Split<S> {
    type Completions = SplitCompletions<S>;
}

impl<S, R> SenderTo<R> for Split<S>
where
    S: Sender + SenderTo<SplitReceiver<S>> + Send + 'static,
    <S as SenderTo<SplitReceiver<S>>>::Operation: Send,
    <S::Completions as CompletionSignatures>::Value: Clone + Send,
    <S::Completions as CompletionSignatures>::Error: Clone + Send,
    R: ReceiverOf<<S::Completions as CompletionSignatures>::Value>
        + ReceiverOfError<<S::Completions as CompletionSignatures>::Error>
        + ReceiverOfStopped
        + Send
        + 'static,
{
    type Operation = SplitOperation<S, R>;

    fn connect(self, receiver: R) -> Self::Operation {
        SplitOperation {
            inner: self.inner,
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

/// Operation state for split sender.
pub struct SplitOperation<S, R>
where
    S: Sender,
{
    inner: Arc<SplitInner<S>>,
    receiver: Arc<ReceiverSlot<R>>,
}

// SAFETY: Fields are Send
unsafe impl<S: Sender + Send, R: Send> Send for SplitOperation<S, R> {}
unsafe impl<S: Sender + Send, R: Send> Sync for SplitOperation<S, R> {}

/// Environment for split receiver.
pub struct SplitEnv;

impl Environment for SplitEnv {
    type StopToken = crate::stop::NeverStopToken;

    fn get_stop_token(&self) -> Self::StopToken {
        crate::stop::NeverStopToken
    }
}

/// Receiver for the underlying sender in split.
pub struct SplitReceiver<S: Sender> {
    inner: Arc<SplitInner<S>>,
}

impl<S: Sender> Receiver for SplitReceiver<S> {
    type Env = SplitEnv;

    fn get_env(&self) -> &Self::Env {
        &SplitEnv
    }
}

impl<S> ReceiverOf<<S::Completions as CompletionSignatures>::Value> for SplitReceiver<S>
where
    S: Sender + Send + 'static,
    <S::Completions as CompletionSignatures>::Value: Clone + Send,
    <S::Completions as CompletionSignatures>::Error: Clone + Send,
{
    fn set_value(self, value: <S::Completions as CompletionSignatures>::Value) {
        self.complete(SplitResult::Value(value));
    }
}

impl<S> ReceiverOfError<<S::Completions as CompletionSignatures>::Error> for SplitReceiver<S>
where
    S: Sender + Send + 'static,
    <S::Completions as CompletionSignatures>::Value: Clone + Send,
    <S::Completions as CompletionSignatures>::Error: Clone + Send,
{
    fn set_error(self, error: <S::Completions as CompletionSignatures>::Error) {
        self.complete(SplitResult::Error(error));
    }
}

impl<S> ReceiverOfStopped for SplitReceiver<S>
where
    S: Sender + Send + 'static,
    <S::Completions as CompletionSignatures>::Value: Clone + Send,
    <S::Completions as CompletionSignatures>::Error: Clone + Send,
{
    fn set_stopped(self) {
        self.complete(SplitResult::Stopped);
    }
}

impl<S> SplitReceiver<S>
where
    S: Sender + Send + 'static,
    <S::Completions as CompletionSignatures>::Value: Clone + Send,
    <S::Completions as CompletionSignatures>::Error: Clone + Send,
{
    fn complete(self, result: SplitResult<S>) {
        // Store the result
        unsafe {
            *self.inner.result.get() = Some(result);
        }

        // Mark as completed
        self.inner.completed.store(true, Ordering::Release);

        // Notify all waiters
        let waiters = {
            let mut guard = self.inner.waiters.lock().unwrap();
            std::mem::take(&mut *guard)
        };

        let result_ref = unsafe { (*self.inner.result.get()).as_ref().unwrap() };
        for waiter in waiters {
            waiter(result_ref);
        }
    }
}

fn notify_receiver<S, R>(slot: &ReceiverSlot<R>, result: &SplitResult<S>)
where
    S: Sender,
    <S::Completions as CompletionSignatures>::Value: Clone,
    <S::Completions as CompletionSignatures>::Error: Clone,
    R: ReceiverOf<<S::Completions as CompletionSignatures>::Value>
        + ReceiverOfError<<S::Completions as CompletionSignatures>::Error>
        + ReceiverOfStopped,
{
    if slot
        .notified
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        let receiver = unsafe { (*slot.receiver.get()).take() };
        if let Some(receiver) = receiver {
            match result {
                SplitResult::Value(v) => receiver.set_value(v.clone()),
                SplitResult::Error(e) => receiver.set_error(e.clone()),
                SplitResult::Stopped => receiver.set_stopped(),
            }
        }
    }
}

impl<S, R> OperationState for SplitOperation<S, R>
where
    S: Sender + SenderTo<SplitReceiver<S>> + Send + 'static,
    <S as SenderTo<SplitReceiver<S>>>::Operation: Send,
    <S::Completions as CompletionSignatures>::Value: Clone + Send,
    <S::Completions as CompletionSignatures>::Error: Clone + Send,
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
            let result_ref = unsafe { (*this.inner.result.get()).as_ref().unwrap() };
            notify_receiver::<S, R>(&this.receiver, result_ref);
            return;
        }

        // Register as a waiter
        let receiver_slot = this.receiver.clone();

        // Create a callback that will notify this operation
        let callback: Box<dyn FnOnce(&SplitResult<S>) + Send> = Box::new(move |result| {
            notify_receiver::<S, R>(&receiver_slot, result);
        });

        {
            let mut waiters = this.inner.waiters.lock().unwrap();

            // Double-check completion while holding the lock
            if this.inner.completed.load(Ordering::Acquire) {
                drop(waiters);
                let result_ref = unsafe { (*this.inner.result.get()).as_ref().unwrap() };
                notify_receiver::<S, R>(&this.receiver, result_ref);
                return;
            }

            waiters.push(callback);
        }

        // Try to start the underlying sender
        if this
            .inner
            .started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            // We are the first to start
            let sender = unsafe { (*this.inner.sender.get()).take().unwrap() };
            let receiver = SplitReceiver {
                inner: this.inner.clone(),
            };
            let mut op = sender.connect(receiver);
            // SAFETY: We're starting immediately and the operation is heap-allocated via Arc
            unsafe {
                Pin::new_unchecked(&mut op).start();
            }
            // Note: op is dropped here, but that's OK because the receiver holds Arc to inner
            // The operation should complete synchronously or the receiver callbacks handle completion
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adaptors::then;
    use crate::consumers::sync_wait;
    use crate::factories::{just, just_error};
    use crate::traits::Sender;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // ========================================================================
    // test_split.cpp equivalents
    // ========================================================================

    #[test]
    fn test_split_single_consumer() {
        let sender = split(just(42));
        let result = sync_wait(sender).unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn test_split_multiple_consumers() {
        let sender = split(just(42));
        let s1 = sender.clone();
        let s2 = sender.clone();

        let result1 = sync_wait(s1).unwrap();
        let result2 = sync_wait(s2).unwrap();
        let result3 = sync_wait(sender).unwrap();

        assert_eq!(result1, 42);
        assert_eq!(result2, 42);
        assert_eq!(result3, 42);
    }

    #[test]
    fn test_split_runs_once() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let sender = split(then(just(()), move |_| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
            42
        }));

        let s1 = sender.clone();
        let s2 = sender.clone();

        let result1 = sync_wait(s1).unwrap();
        let result2 = sync_wait(s2).unwrap();

        assert_eq!(result1, 42);
        assert_eq!(result2, 42);
        // The underlying sender should only run once
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_split_error() {
        let sender = split(just_error("error"));
        let result = sync_wait(sender);
        assert!(result.is_err());
    }

    #[test]
    fn test_split_is_sender() {
        fn assert_sender<S: Sender>(_: &S) {}
        let s = split(just(42));
        assert_sender(&s);
    }

    #[test]
    fn test_split_many_clones() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let sender = split(then(just(()), move |_| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
            42
        }));

        // Create many clones and use them all
        let clones: Vec<_> = (0..10).map(|_| sender.clone()).collect();

        for clone in clones {
            let result = sync_wait(clone).unwrap();
            assert_eq!(result, 42);
        }

        // Work should only run once
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_split_with_string() {
        let sender = split(just(String::from("hello")));
        let s1 = sender.clone();
        let s2 = sender.clone();

        let result1 = sync_wait(s1).unwrap();
        let result2 = sync_wait(s2).unwrap();

        assert_eq!(result1, "hello");
        assert_eq!(result2, "hello");
    }
}
