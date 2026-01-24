//! Stop callback utilities.

use std::sync::atomic::{AtomicBool, Ordering};

/// A stop callback that wraps a closure.
///
/// This is the concrete callback type used by `InplaceStopSource`.
pub struct StopCallback<F> {
    callback: Option<F>,
    invoked: AtomicBool,
}

impl<F> StopCallback<F> {
    /// Create a new stop callback.
    pub fn new(callback: F) -> Self {
        Self {
            callback: Some(callback),
            invoked: AtomicBool::new(false),
        }
    }

    /// Check if the callback was invoked.
    pub fn was_invoked(&self) -> bool {
        self.invoked.load(Ordering::Acquire)
    }
}

impl<F: FnOnce()> StopCallback<F> {
    /// Execute the callback if it hasn't been executed yet.
    pub fn execute(&mut self) {
        if let Some(callback) = self.callback.take() {
            self.invoked.store(true, Ordering::Release);
            callback();
        }
    }
}

/// A type-erased stop callback for use in intrusive linked lists.
pub(crate) trait ErasedCallback: Send {
    /// Execute the callback.
    fn execute(&mut self);
}

impl<F: FnOnce() + Send> ErasedCallback for StopCallback<F> {
    fn execute(&mut self) {
        StopCallback::execute(self);
    }
}
