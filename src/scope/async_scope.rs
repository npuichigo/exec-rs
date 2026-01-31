//! AsyncScope - full async scope with nest, spawn, when_empty.
//!
//! AsyncScope provides structured concurrency by tracking sender lifetimes
//! and ensuring all nested work completes before the scope is destroyed.

use crate::stop::{InplaceStopSource, InplaceStopToken, StopSource};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// A full async scope for structured concurrency.
///
/// AsyncScope tracks nested senders and provides:
/// - `nest(sender)` - wrap a sender to track it in the scope
/// - `spawn(sender)` - fire-and-forget within the scope
/// - `when_empty()` - sender that completes when all work is done
///
/// # Example
///
/// ```rust,ignore
/// use exec_rs::scope::AsyncScope;
/// use exec_rs::factories::just;
/// use exec_rs::consumers::sync_wait;
///
/// let scope = AsyncScope::new();
///
/// // Nest tracks the sender
/// let nested = scope.nest(just(42));
/// let result = sync_wait(nested).unwrap();
/// assert_eq!(result, 42);
///
/// // Wait for scope to be empty
/// sync_wait(scope.when_empty()).unwrap();
/// ```
pub struct AsyncScope {
    inner: Arc<AsyncScopeInner>,
}

pub(crate) struct AsyncScopeInner {
    /// Number of active nested operations.
    pub(crate) active: AtomicUsize,
    /// Stop source for cancelling all operations.
    pub(crate) stop_source: InplaceStopSource,
    /// Waiters to notify when scope becomes empty.
    pub(crate) waiters: Mutex<VecDeque<Box<dyn FnOnce() + Send>>>,
}

impl AsyncScope {
    /// Create a new async scope.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(AsyncScopeInner {
                active: AtomicUsize::new(0),
                stop_source: InplaceStopSource::new(),
                waiters: Mutex::new(VecDeque::new()),
            }),
        }
    }

    /// Nest a sender within this scope.
    ///
    /// The returned sender tracks its lifetime within the scope.
    /// When started, it increments the active count. When completed,
    /// it decrements the count.
    pub fn nest<S>(&self, sender: S) -> super::Nest<S> {
        super::Nest::new(sender, self.inner.clone())
    }

    /// Spawn a sender within this scope (fire-and-forget).
    ///
    /// The sender is nested and started immediately. The result is discarded.
    pub fn spawn<S>(&self, sender: S)
    where
        S: crate::traits::Sender
            + crate::traits::SenderTo<super::NestReceiver<crate::consumers::DetachedReceiver>>
            + 'static,
        <S as crate::traits::SenderTo<super::NestReceiver<crate::consumers::DetachedReceiver>>>::Operation: Send + 'static,
    {
        use crate::consumers::start_detached;
        start_detached(self.nest(sender));
    }

    /// Returns a sender that completes when the scope is empty.
    pub fn when_empty(&self) -> AsyncScopeWhenEmpty {
        AsyncScopeWhenEmpty {
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

    /// Get the current count of active operations.
    pub fn active_count(&self) -> usize {
        self.inner.active.load(Ordering::Acquire)
    }

    pub(crate) fn inner(&self) -> &Arc<AsyncScopeInner> {
        &self.inner
    }
}

impl Default for AsyncScope {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for AsyncScope {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

/// Notify waiters when the active count reaches zero.
pub(crate) fn notify_waiters_if_empty(inner: &Arc<AsyncScopeInner>) {
    if inner.active.load(Ordering::Acquire) == 0 {
        let mut waiters = inner.waiters.lock().unwrap();
        while let Some(waiter) = waiters.pop_front() {
            drop(waiters);
            waiter();
            waiters = inner.waiters.lock().unwrap();
        }
    }
}

/// A sender that completes when the async scope is empty.
pub struct AsyncScopeWhenEmpty {
    scope: Arc<AsyncScopeInner>,
}

impl crate::traits::Sender for AsyncScopeWhenEmpty {
    type Completions = super::WhenEmptyCompletions;
}

impl<R> crate::traits::SenderTo<R> for AsyncScopeWhenEmpty
where
    R: crate::traits::ReceiverOf<()> + Send + 'static,
{
    type Operation = AsyncScopeWhenEmptyOp<R>;

    fn connect(self, receiver: R) -> Self::Operation {
        AsyncScopeWhenEmptyOp {
            scope: self.scope,
            receiver: std::cell::UnsafeCell::new(Some(receiver)),
            started: false,
        }
    }
}

/// Operation state for AsyncScopeWhenEmpty.
pub struct AsyncScopeWhenEmptyOp<R> {
    scope: Arc<AsyncScopeInner>,
    receiver: std::cell::UnsafeCell<Option<R>>,
    started: bool,
}

unsafe impl<R: Send> Send for AsyncScopeWhenEmptyOp<R> {}

impl<R: crate::traits::ReceiverOf<()> + Send + 'static> crate::traits::OperationState
    for AsyncScopeWhenEmptyOp<R>
{
    fn start(self: std::pin::Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };

        if this.started {
            return;
        }
        this.started = true;

        if this.scope.active.load(Ordering::Acquire) == 0 {
            let receiver = unsafe { (*this.receiver.get()).take() };
            if let Some(r) = receiver {
                r.set_value(());
            }
        } else {
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
    fn test_async_scope_empty() {
        let scope = AsyncScope::new();
        let result = sync_wait(scope.when_empty()).unwrap();
        assert_eq!(result, ());
    }

    #[test]
    fn test_async_scope_nest() {
        let scope = AsyncScope::new();
        let result = sync_wait(scope.nest(just(42))).unwrap();
        assert_eq!(result, 42);
        assert_eq!(scope.active_count(), 0);
    }

    #[test]
    fn test_async_scope_spawn() {
        let scope = AsyncScope::new();
        scope.spawn(just(1));
        scope.spawn(just(2));
        // With inline execution, spawns complete synchronously
        let result = sync_wait(scope.when_empty()).unwrap();
        assert_eq!(result, ());
    }
}
