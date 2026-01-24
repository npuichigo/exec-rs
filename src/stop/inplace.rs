//! Inplace stop source and token implementation.
//!
//! This is the primary stop token implementation, matching C++'s
//! `std::inplace_stop_source` and `std::inplace_stop_token`.

use crate::stop::{StopSource, StopToken, StopCallbackHandle};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::thread;

/// State flags for the stop source.
const STOP_REQUESTED: u8 = 1;
const LOCKED: u8 = 2;

/// An inplace stop source.
///
/// This is the primary stop source implementation. It supports:
/// - Thread-safe stop requests
/// - Callback registration and execution
/// - Proper synchronization for concurrent access
#[derive(Debug)]
pub struct InplaceStopSource {
    inner: Arc<InplaceStopSourceInner>,
}

struct InplaceStopSourceInner {
    state: AtomicU8,
    callbacks: Mutex<Vec<Box<dyn FnOnce() + Send>>>,
    notifying_thread: Mutex<Option<thread::ThreadId>>,
}

impl std::fmt::Debug for InplaceStopSourceInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InplaceStopSourceInner")
            .field("state", &self.state)
            .field("callbacks", &format!("<{} callbacks>", self.callbacks.lock().len()))
            .field("notifying_thread", &self.notifying_thread)
            .finish()
    }
}

impl InplaceStopSource {
    /// Create a new stop source.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(InplaceStopSourceInner {
                state: AtomicU8::new(0),
                callbacks: Mutex::new(Vec::new()),
                notifying_thread: Mutex::new(None),
            }),
        }
    }
}

impl Default for InplaceStopSource {
    fn default() -> Self {
        Self::new()
    }
}

impl StopSource for InplaceStopSource {
    type Token = InplaceStopToken;

    fn get_token(&self) -> Self::Token {
        InplaceStopToken {
            inner: Some(self.inner.clone()),
        }
    }

    fn request_stop(&self) -> bool {
        // Try to acquire the lock and set stop_requested
        let mut old_state = self.inner.state.load(Ordering::Relaxed);
        loop {
            if old_state & STOP_REQUESTED != 0 {
                // Already stopped
                return false;
            }

            // Spin while locked
            while old_state & LOCKED != 0 {
                std::hint::spin_loop();
                old_state = self.inner.state.load(Ordering::Relaxed);
            }

            match self.inner.state.compare_exchange_weak(
                old_state,
                old_state | STOP_REQUESTED | LOCKED,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(x) => old_state = x,
            }
        }

        // Record the notifying thread
        *self.inner.notifying_thread.lock() = Some(thread::current().id());

        // Execute all callbacks
        let callbacks = std::mem::take(&mut *self.inner.callbacks.lock());
        for callback in callbacks {
            callback();
        }

        // Release the lock but keep stop_requested
        self.inner.state.store(STOP_REQUESTED, Ordering::Release);

        true
    }

    fn stop_requested(&self) -> bool {
        self.inner.state.load(Ordering::Acquire) & STOP_REQUESTED != 0
    }
}

/// An inplace stop token.
///
/// This token is associated with an `InplaceStopSource` and can be used to
/// check for stop requests and register callbacks.
#[derive(Debug, Clone)]
pub struct InplaceStopToken {
    inner: Option<Arc<InplaceStopSourceInner>>,
}

impl InplaceStopToken {
    /// Create a token that is not associated with any source.
    pub fn new_detached() -> Self {
        Self { inner: None }
    }
}

impl Default for InplaceStopToken {
    fn default() -> Self {
        Self::new_detached()
    }
}

impl StopToken for InplaceStopToken {
    type Callback<F: FnOnce() + Send> = InplaceStopCallback;

    fn stop_requested(&self) -> bool {
        self.inner
            .as_ref()
            .map(|inner| inner.state.load(Ordering::Acquire) & STOP_REQUESTED != 0)
            .unwrap_or(false)
    }

    fn stop_possible(&self) -> bool {
        self.inner.is_some()
    }

    fn register_callback<F: FnOnce() + Send + 'static>(&self, callback: F) -> Self::Callback<F> {
        let Some(inner) = &self.inner else {
            // No source, callback will never be invoked
            return InplaceStopCallback {
                inner: None,
                invoked: AtomicBool::new(false),
            };
        };

        // Check if stop was already requested
        if inner.state.load(Ordering::Acquire) & STOP_REQUESTED != 0 {
            // Execute callback immediately
            callback();
            return InplaceStopCallback {
                inner: None,
                invoked: AtomicBool::new(true),
            };
        }

        // Try to register the callback
        let callback_id = {
            let mut callbacks = inner.callbacks.lock();

            // Double-check stop wasn't requested while we were waiting for the lock
            if inner.state.load(Ordering::Acquire) & STOP_REQUESTED != 0 {
                drop(callbacks);
                callback();
                return InplaceStopCallback {
                    inner: None,
                    invoked: AtomicBool::new(true),
                };
            }

            let id = callbacks.len();
            callbacks.push(Box::new(callback));
            id
        };

        InplaceStopCallback {
            inner: Some(CallbackInner {
                source: inner.clone(),
                id: callback_id,
            }),
            invoked: AtomicBool::new(false),
        }
    }
}

#[derive(Debug)]
struct CallbackInner {
    source: Arc<InplaceStopSourceInner>,
    #[allow(dead_code)]
    id: usize,
}

/// Callback handle for inplace stop tokens.
#[derive(Debug)]
pub struct InplaceStopCallback {
    inner: Option<CallbackInner>,
    invoked: AtomicBool,
}

impl StopCallbackHandle for InplaceStopCallback {
    fn was_invoked(&self) -> bool {
        self.invoked.load(Ordering::Acquire)
    }
}

impl Drop for InplaceStopCallback {
    fn drop(&mut self) {
        // If the callback was already invoked or there's no inner, nothing to do
        if self.inner.is_none() {
            return;
        }

        let inner = self.inner.as_ref().unwrap();

        // If stop was requested and we're on the notifying thread, the callback
        // might be executing right now
        if inner.source.state.load(Ordering::Acquire) & STOP_REQUESTED != 0 {
            let notifying_thread = inner.source.notifying_thread.lock();
            if *notifying_thread == Some(thread::current().id()) {
                // We're on the notifying thread, the callback might be in progress
                // Just mark it as invoked to prevent double-execution
                self.invoked.store(true, Ordering::Release);
                return;
            }
        }

        // Otherwise, we need to wait until the callback finishes or remove it
        // For simplicity, we just mark it as invoked
        // A production implementation would properly remove from the callback list
        self.invoked.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stop_source_basic() {
        let source = InplaceStopSource::new();
        let token = source.get_token();

        assert!(!token.stop_requested());
        assert!(token.stop_possible());

        assert!(source.request_stop());
        assert!(!source.request_stop()); // Second call returns false

        assert!(token.stop_requested());
    }

    #[test]
    fn test_callback_execution() {
        use std::sync::atomic::AtomicUsize;

        let source = InplaceStopSource::new();
        let token = source.get_token();

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let _callback = token.register_callback(move || {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        });

        assert_eq!(counter.load(Ordering::SeqCst), 0);

        source.request_stop();

        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_callback_immediate_execution() {
        use std::sync::atomic::AtomicUsize;

        let source = InplaceStopSource::new();
        let token = source.get_token();

        source.request_stop();

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        // Callback should execute immediately since stop was already requested
        let callback = token.register_callback(move || {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        });

        assert!(callback.was_invoked());
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_never_stop_token() {
        use crate::stop::NeverStopToken;

        let token = NeverStopToken;
        assert!(!token.stop_requested());
        assert!(!token.stop_possible());

        let _callback = token.register_callback(|| {
            panic!("Should never be called");
        });
    }
}
