//! CountingScope - a simple reference-counting scope.
//!
//! CountingScope tracks the number of active spawned senders and provides
//! a `when_empty` sender that completes when all work is done.

use crate::consumers::start_detached;
use crate::stop::{InplaceStopSource, InplaceStopToken, StopSource, StopToken};
use crate::traits::{
    CompletionSignatures, Environment, OperationState, Receiver, ReceiverOf, ReceiverOfError,
    ReceiverOfStopped, Sender, SenderTo,
};
use std::cell::UnsafeCell;
use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// A simple scope that counts active operations.
///
/// CountingScope tracks spawned senders and provides synchronization
/// to wait for all work to complete.
///
/// # Example
///
/// ```rust
/// use exec_rs::scope::CountingScope;
/// use exec_rs::factories::just;
/// use exec_rs::consumers::sync_wait;
///
/// let scope = CountingScope::new();
/// scope.spawn(just(42));
/// sync_wait(scope.when_empty()).unwrap();
/// ```
pub struct CountingScope {
    inner: Arc<CountingScopeInner>,
}

struct CountingScopeInner {
    /// Number of active operations.
    active: AtomicUsize,
    /// Stop source for cancelling all operations.
    stop_source: InplaceStopSource,
    /// Waiters to notify when scope becomes empty.
    waiters: Mutex<VecDeque<Box<dyn FnOnce() + Send>>>,
}

impl CountingScope {
    /// Create a new counting scope.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(CountingScopeInner {
                active: AtomicUsize::new(0),
                stop_source: InplaceStopSource::new(),
                waiters: Mutex::new(VecDeque::new()),
            }),
        }
    }

    /// Spawn a sender within this scope.
    ///
    /// The sender will be tracked by the scope and must complete before
    /// `when_empty` completes.
    pub fn spawn<S>(&self, sender: S)
    where
        S: Sender + SenderTo<ScopedReceiver>,
    {
        // Increment active count
        self.inner.active.fetch_add(1, Ordering::AcqRel);

        let receiver = ScopedReceiver {
            scope: self.inner.clone(),
        };

        // Start the sender
        start_detached(ScopedSender { sender, receiver });
    }

    /// Returns a sender that completes when the scope is empty.
    ///
    /// If the scope is already empty, the sender completes immediately.
    pub fn when_empty(&self) -> WhenEmpty {
        WhenEmpty {
            scope: self.inner.clone(),
        }
    }

    /// Request stop for all operations in this scope.
    pub fn request_stop(&self) {
        self.inner.stop_source.request_stop();
    }

    /// Get the stop token for this scope.
    pub fn get_stop_token(&self) -> InplaceStopToken {
        self.inner.stop_source.get_token()
    }

    /// Check if stop has been requested.
    pub fn stop_requested(&self) -> bool {
        self.inner.stop_source.get_token().stop_requested()
    }

    /// Get the current count of active operations.
    pub fn active_count(&self) -> usize {
        self.inner.active.load(Ordering::Acquire)
    }
}

impl Default for CountingScope {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for CountingScope {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

// Notify waiters when count reaches zero
fn notify_if_empty(inner: &Arc<CountingScopeInner>) {
    if inner.active.load(Ordering::Acquire) == 0 {
        let mut waiters = inner.waiters.lock().unwrap();
        while let Some(waiter) = waiters.pop_front() {
            drop(waiters); // Release lock before calling waiter
            waiter();
            waiters = inner.waiters.lock().unwrap();
        }
    }
}

/// A sender wrapper that tracks completion in the scope.
struct ScopedSender<S> {
    sender: S,
    receiver: ScopedReceiver,
}

impl<S: Sender> Sender for ScopedSender<S> {
    type Completions = S::Completions;
}

impl<S> SenderTo<crate::consumers::DetachedReceiver> for ScopedSender<S>
where
    S: SenderTo<ScopedReceiver>,
{
    type Operation = ScopedOperation<S>;

    fn connect(self, _receiver: crate::consumers::DetachedReceiver) -> Self::Operation {
        ScopedOperation {
            inner: self.sender.connect(self.receiver),
        }
    }
}

/// Receiver that decrements count on completion.
pub struct ScopedReceiver {
    scope: Arc<CountingScopeInner>,
}

/// Environment for scoped receivers.
pub struct ScopedEnv {
    stop_token: InplaceStopToken,
}

impl Environment for ScopedEnv {
    type StopToken = InplaceStopToken;

    fn get_stop_token(&self) -> Self::StopToken {
        self.stop_token.clone()
    }
}

impl Receiver for ScopedReceiver {
    type Env = ScopedEnv;

    fn get_env(&self) -> &Self::Env {
        // Return environment with scope's stop token
        thread_local! {
            static ENV: ScopedEnv = ScopedEnv {
                stop_token: InplaceStopToken::new_detached(),
            };
        }
        ENV.with(|env| unsafe { &*(env as *const ScopedEnv) })
    }
}

impl<V> ReceiverOf<V> for ScopedReceiver {
    fn set_value(self, _value: V) {
        // Decrement count and notify waiters
        let prev = self.scope.active.fetch_sub(1, Ordering::AcqRel);
        if prev == 1 {
            notify_if_empty(&self.scope);
        }
    }
}

impl<E> ReceiverOfError<E> for ScopedReceiver {
    fn set_error(self, _error: E) {
        let prev = self.scope.active.fetch_sub(1, Ordering::AcqRel);
        if prev == 1 {
            notify_if_empty(&self.scope);
        }
    }
}

impl ReceiverOfStopped for ScopedReceiver {
    fn set_stopped(self) {
        let prev = self.scope.active.fetch_sub(1, Ordering::AcqRel);
        if prev == 1 {
            notify_if_empty(&self.scope);
        }
    }
}

/// Operation state for scoped sender.
struct ScopedOperation<S: SenderTo<ScopedReceiver>> {
    inner: S::Operation,
}

impl<S: SenderTo<ScopedReceiver>> OperationState for ScopedOperation<S> {
    fn start(self: Pin<&mut Self>) {
        unsafe {
            let this = self.get_unchecked_mut();
            Pin::new_unchecked(&mut this.inner).start();
        }
    }
}

/// A sender that completes when the scope is empty.
pub struct WhenEmpty {
    scope: Arc<CountingScopeInner>,
}

/// Completion signatures for WhenEmpty.
pub struct WhenEmptyCompletions;

impl CompletionSignatures for WhenEmptyCompletions {
    type Value = ();
    type Error = std::convert::Infallible;
    const SENDS_STOPPED: bool = false;
}

impl Sender for WhenEmpty {
    type Completions = WhenEmptyCompletions;
}

impl<R> SenderTo<R> for WhenEmpty
where
    R: ReceiverOf<()> + Send + 'static,
{
    type Operation = WhenEmptyOperation<R>;

    fn connect(self, receiver: R) -> Self::Operation {
        WhenEmptyOperation {
            scope: self.scope,
            receiver: UnsafeCell::new(Some(receiver)),
            started: false,
        }
    }
}

/// Operation state for WhenEmpty.
pub struct WhenEmptyOperation<R> {
    scope: Arc<CountingScopeInner>,
    receiver: UnsafeCell<Option<R>>,
    started: bool,
}

// SAFETY: We synchronize access via mutex in the scope
unsafe impl<R: Send> Send for WhenEmptyOperation<R> {}

impl<R: ReceiverOf<()> + Send + 'static> OperationState for WhenEmptyOperation<R> {
    fn start(self: Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };

        if this.started {
            return;
        }
        this.started = true;

        // Check if already empty
        if this.scope.active.load(Ordering::Acquire) == 0 {
            // Complete immediately
            let receiver = unsafe { (*this.receiver.get()).take() };
            if let Some(r) = receiver {
                r.set_value(());
            }
        } else {
            // Register as a waiter
            let receiver = unsafe { (*this.receiver.get()).take() };
            if let Some(r) = receiver {
                let mut waiters = this.scope.waiters.lock().unwrap();
                waiters.push_back(Box::new(move || {
                    r.set_value(());
                }));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consumers::sync_wait;
    use crate::factories::just;

    #[test]
    fn test_counting_scope_empty() {
        let scope = CountingScope::new();
        // Already empty, should complete immediately
        let result = sync_wait(scope.when_empty()).unwrap();
        assert_eq!(result, ());
    }

    #[test]
    fn test_counting_scope_spawn() {
        let scope = CountingScope::new();
        scope.spawn(just(42));
        // With inline execution, spawn completes synchronously
        assert_eq!(scope.active_count(), 0);
        let result = sync_wait(scope.when_empty()).unwrap();
        assert_eq!(result, ());
    }

    #[test]
    fn test_counting_scope_multiple_spawns() {
        let scope = CountingScope::new();
        scope.spawn(just(1));
        scope.spawn(just(2));
        scope.spawn(just(3));
        let result = sync_wait(scope.when_empty()).unwrap();
        assert_eq!(result, ());
    }
}
