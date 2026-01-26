//! Cleanup functionality for async code.
//!
//! Provides `at_coroutine_exit` and related utilities for guaranteed
//! cleanup when async tasks complete or are cancelled.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

/// A future that runs cleanup code when the inner future completes or is dropped.
///
/// This ensures cleanup runs whether the future completes normally,
/// with an error, or is cancelled/dropped.
///
/// # Example
///
/// ```rust,ignore
/// use exec_rs::coro::at_coroutine_exit;
///
/// async fn example() {
///     let result = at_coroutine_exit(
///         async { 42 },
///         || println!("Cleanup!"),
///     ).await;
///     assert_eq!(result, 42);
/// }
/// ```
pub fn at_coroutine_exit<F, C, T>(future: F, cleanup: C) -> AtCoroutineExit<F, C>
where
    F: Future<Output = T>,
    C: FnOnce(),
{
    AtCoroutineExit {
        future,
        cleanup: Some(cleanup),
    }
}

/// A future wrapper that guarantees cleanup execution.
pub struct AtCoroutineExit<F, C: FnOnce()> {
    future: F,
    cleanup: Option<C>,
}

impl<F, C, T> Future for AtCoroutineExit<F, C>
where
    F: Future<Output = T>,
    C: FnOnce(),
{
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // SAFETY: We never move out of the future field
        let this = unsafe { self.get_unchecked_mut() };
        let future = unsafe { Pin::new_unchecked(&mut this.future) };

        match future.poll(cx) {
            Poll::Ready(value) => {
                // Run cleanup on completion
                if let Some(cleanup) = this.cleanup.take() {
                    cleanup();
                }
                Poll::Ready(value)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<F, C> Drop for AtCoroutineExit<F, C>
where
    C: FnOnce(),
{
    fn drop(&mut self) {
        // Run cleanup on drop (cancellation)
        if let Some(cleanup) = self.cleanup.take() {
            cleanup();
        }
    }
}

/// A scope guard that runs cleanup code when dropped.
///
/// Unlike `at_coroutine_exit`, this doesn't wrap a future but instead
/// provides RAII-style cleanup for any scope.
///
/// # Example
///
/// ```rust
/// use exec_rs::coro::scope_exit;
///
/// fn example() {
///     let _guard = scope_exit(|| println!("Cleanup!"));
///     // ... do work ...
/// } // Cleanup runs here
/// ```
pub fn scope_exit<F: FnOnce()>(cleanup: F) -> ScopeExit<F> {
    ScopeExit {
        cleanup: Some(cleanup),
    }
}

/// A scope guard that runs cleanup on drop.
pub struct ScopeExit<F: FnOnce()> {
    cleanup: Option<F>,
}

impl<F> ScopeExit<F>
where
    F: FnOnce(),
{
    /// Disarm the guard, preventing cleanup from running.
    pub fn disarm(&mut self) {
        self.cleanup = None;
    }

    /// Run cleanup immediately and disarm the guard.
    pub fn run(mut self) {
        if let Some(cleanup) = self.cleanup.take() {
            cleanup();
        }
    }
}

impl<F> Drop for ScopeExit<F>
where
    F: FnOnce(),
{
    fn drop(&mut self) {
        if let Some(cleanup) = self.cleanup.take() {
            cleanup();
        }
    }
}

/// A deferred cleanup handle that can be attached to async operations.
///
/// This is useful when you need to register cleanup that should run
/// when a particular operation completes or is cancelled.
///
/// Note: This uses boxed closures to allow heterogeneous cleanup functions.
pub struct DeferredCleanup {
    cleanups: Vec<Box<dyn FnOnce() + Send>>,
}

impl DeferredCleanup {
    /// Create a new deferred cleanup handle.
    pub fn new() -> Self {
        Self {
            cleanups: Vec::new(),
        }
    }

    /// Register a cleanup function.
    pub fn register<F: FnOnce() + Send + 'static>(&mut self, cleanup: F) {
        self.cleanups.push(Box::new(cleanup));
    }

    /// Run all registered cleanups in reverse order.
    pub fn run_all(mut self) {
        while let Some(cleanup) = self.cleanups.pop() {
            cleanup();
        }
    }
}

impl Default for DeferredCleanup {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for DeferredCleanup {
    fn drop(&mut self) {
        while let Some(cleanup) = self.cleanups.pop() {
            cleanup();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::task::{RawWaker, RawWakerVTable, Waker};

    fn block_on<F: Future>(f: F) -> F::Output {
        static VTABLE: RawWakerVTable = RawWakerVTable::new(
            |_| RawWaker::new(std::ptr::null(), &VTABLE),
            |_| {},
            |_| {},
            |_| {},
        );

        let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
        let mut cx = Context::from_waker(&waker);
        let mut f = std::pin::pin!(f);

        loop {
            match f.as_mut().poll(&mut cx) {
                Poll::Ready(result) => return result,
                Poll::Pending => {
                    std::thread::yield_now();
                }
            }
        }
    }

    #[test]
    fn test_at_coroutine_exit_normal_completion() {
        let cleanup_ran = Arc::new(AtomicBool::new(false));
        let cleanup_ran_clone = cleanup_ran.clone();

        let result = block_on(at_coroutine_exit(async { 42 }, move || {
            cleanup_ran_clone.store(true, Ordering::SeqCst);
        }));

        assert_eq!(result, 42);
        assert!(cleanup_ran.load(Ordering::SeqCst));
    }

    #[test]
    fn test_at_coroutine_exit_drop() {
        let cleanup_ran = Arc::new(AtomicBool::new(false));
        let cleanup_ran_clone = cleanup_ran.clone();

        {
            let _future = at_coroutine_exit(async { 42 }, move || {
                cleanup_ran_clone.store(true, Ordering::SeqCst);
            });
            // Drop without polling
        }

        assert!(cleanup_ran.load(Ordering::SeqCst));
    }

    #[test]
    fn test_scope_exit() {
        let cleanup_ran = Arc::new(AtomicBool::new(false));
        let cleanup_ran_clone = cleanup_ran.clone();

        {
            let _guard = scope_exit(move || {
                cleanup_ran_clone.store(true, Ordering::SeqCst);
            });
        }

        assert!(cleanup_ran.load(Ordering::SeqCst));
    }

    #[test]
    fn test_scope_exit_disarm() {
        let cleanup_ran = Arc::new(AtomicBool::new(false));
        let cleanup_ran_clone = cleanup_ran.clone();

        {
            let mut guard = scope_exit(move || {
                cleanup_ran_clone.store(true, Ordering::SeqCst);
            });
            guard.disarm();
        }

        assert!(!cleanup_ran.load(Ordering::SeqCst));
    }

    #[test]
    fn test_deferred_cleanup_order() {
        let order = Arc::new(AtomicUsize::new(0));
        let mut deferred: DeferredCleanup = DeferredCleanup::new();

        let order1 = order.clone();
        let order2 = order.clone();
        let order3 = order.clone();

        // Register cleanups - they run in reverse order (LIFO)
        deferred.register(move || {
            assert_eq!(order1.fetch_add(1, Ordering::SeqCst), 2);
        });
        deferred.register(move || {
            assert_eq!(order2.fetch_add(1, Ordering::SeqCst), 1);
        });
        deferred.register(move || {
            assert_eq!(order3.fetch_add(1, Ordering::SeqCst), 0);
        });

        deferred.run_all();
        assert_eq!(order.load(Ordering::SeqCst), 3);
    }
}
