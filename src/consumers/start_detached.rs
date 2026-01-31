//! Start Detached consumer - fire-and-forget sender execution.
//!
//! `start_detached` starts a sender and immediately returns, without waiting
//! for completion. The sender runs to completion independently.

use crate::stop::NeverStopToken;
use crate::traits::{
    Environment, OperationState, Receiver, ReceiverOf, ReceiverOfError, ReceiverOfStopped, Sender,
    SenderTo,
};
use std::pin::Pin;
use std::sync::Arc;

/// Start a sender and detach, not waiting for completion.
///
/// This is useful for fire-and-forget operations where you don't care
/// about the result.
///
/// # Note
///
/// The sender must complete on its own. If the sender depends on external
/// resources or scheduling, those must remain available.
///
/// # Example
///
/// ```rust
/// use exec_rs::factories::just;
/// use exec_rs::consumers::start_detached;
///
/// // Start a sender and don't wait for completion
/// start_detached(just(42));
/// ```
pub fn start_detached<S>(sender: S)
where
    S: Sender + SenderTo<DetachedReceiver>,
    S::Operation: Send + 'static,
{
    // Box the operation and leak it - it will be cleaned up when completed
    let receiver = DetachedReceiver::new();
    let op = sender.connect(receiver.clone());

    // Store the operation in a Box and leak it
    // The operation will clean itself up via the guard when completed
    let boxed_op = Box::new(DetachedOperation {
        op,
        _guard: receiver.guard.clone(),
    });

    // Leak the box - it will be dropped when the guard is dropped
    let leaked = Box::into_raw(boxed_op);

    // Start the operation
    // SAFETY: The operation is heap-allocated and will remain valid
    unsafe {
        let pinned = Pin::new_unchecked(&mut (*leaked).op);
        pinned.start();
    }

    // Store the pointer in the guard so it can be cleaned up
    receiver.set_operation_ptr::<DetachedOperation<S::Operation>>(leaked as *mut ());
}

/// Holds the operation and cleanup guard together
struct DetachedOperation<Op> {
    op: Op,
    _guard: Arc<DetachedGuard>,
}

/// Guard that cleans up the boxed operation when the receiver completes
struct DetachedGuard {
    /// Pointer to the DetachedOperation, set after creation
    op_ptr: std::cell::UnsafeCell<Option<*mut ()>>,
    /// Function to drop the operation
    drop_fn: std::cell::UnsafeCell<Option<unsafe fn(*mut ())>>,
}

// SAFETY: The guard is only accessed from the completion callback
unsafe impl Send for DetachedGuard {}
unsafe impl Sync for DetachedGuard {}

impl DetachedGuard {
    fn new() -> Self {
        Self {
            op_ptr: std::cell::UnsafeCell::new(None),
            drop_fn: std::cell::UnsafeCell::new(None),
        }
    }

    fn cleanup(&self) {
        // SAFETY: Called once when receiver completes
        unsafe {
            if let (Some(ptr), Some(drop_fn)) = (
                (*self.op_ptr.get()).take(),
                (*self.drop_fn.get()).take(),
            ) {
                drop_fn(ptr);
            }
        }
    }
}

impl Drop for DetachedGuard {
    fn drop(&mut self) {
        self.cleanup();
    }
}

/// Environment for detached receivers.
#[derive(Debug, Clone, Default)]
pub struct DetachedEnv;

impl Environment for DetachedEnv {
    type StopToken = NeverStopToken;

    fn get_stop_token(&self) -> Self::StopToken {
        NeverStopToken
    }
}

/// Receiver that discards all completions and keeps operation alive.
#[derive(Clone)]
pub struct DetachedReceiver {
    guard: Arc<DetachedGuard>,
}

impl DetachedReceiver {
    fn new() -> Self {
        Self {
            guard: Arc::new(DetachedGuard::new()),
        }
    }

    fn set_operation_ptr<Op>(&self, ptr: *mut ()) {
        // SAFETY: Called once during setup
        unsafe {
            *self.guard.op_ptr.get() = Some(ptr);
            *self.guard.drop_fn.get() = Some(drop_boxed_operation::<Op>);
        }
    }
}

unsafe fn drop_boxed_operation<Op>(ptr: *mut ()) {
    // SAFETY: ptr was created by Box::into_raw with the same type
    unsafe {
        let _ = Box::from_raw(ptr as *mut DetachedOperation<Op>);
    }
}

impl Receiver for DetachedReceiver {
    type Env = DetachedEnv;

    fn get_env(&self) -> &Self::Env {
        &DetachedEnv
    }
}

impl<V> ReceiverOf<V> for DetachedReceiver {
    fn set_value(self, _value: V) {
        // Discard the value - guard cleanup happens on drop
    }
}

impl<E> ReceiverOfError<E> for DetachedReceiver {
    fn set_error(self, _error: E) {
        // Discard the error - guard cleanup happens on drop
    }
}

impl ReceiverOfStopped for DetachedReceiver {
    fn set_stopped(self) {
        // Discard the stopped signal - guard cleanup happens on drop
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adaptors::then;
    use crate::factories::just;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[test]
    fn test_start_detached_basic() {
        let completed = Arc::new(AtomicBool::new(false));
        let completed_clone = completed.clone();

        start_detached(then(just(()), move |_| {
            completed_clone.store(true, Ordering::SeqCst);
        }));

        // With inline execution, this completes synchronously
        assert!(completed.load(Ordering::SeqCst));
    }

    #[test]
    fn test_start_detached_value() {
        // Just verify it doesn't panic
        start_detached(just(42));
    }

    #[test]
    fn test_start_detached_unit() {
        start_detached(just(()));
    }
}
