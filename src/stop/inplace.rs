//! Inplace stop source and token implementation.
//!
//! This is the primary stop token implementation, matching C++'s
//! `std::inplace_stop_source` and `std::inplace_stop_token`.
//!
//! Uses an intrusive doubly-linked list for callbacks. Each callback node
//! is heap-allocated (to provide stable addresses for the intrusive list),
//! but the closure is stored inline in the node, avoiding separate boxing
//! of the callback function.

use crate::stop::{StopCallbackHandle, StopSource, StopToken};
use parking_lot::Mutex;
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU8, Ordering};
use std::sync::Arc;
use std::thread;

/// State flags for the stop source.
const STOP_REQUESTED: u8 = 1;
const LOCKED: u8 = 2;

/// State flags for callbacks.
const CB_NOT_REGISTERED: u8 = 0;
const CB_REGISTERED: u8 = 1;
const CB_EXECUTING: u8 = 2;
const CB_DONE: u8 = 3;

/// Type-erased callback node header for intrusive list.
///
/// This structure is embedded at the start of each concrete callback node type.
/// Using `#[repr(C)]` ensures a predictable layout for safe casting.
#[repr(C)]
struct CallbackNodeHeader {
    /// Next node in the list (null if tail).
    next: AtomicPtr<CallbackNodeHeader>,
    /// Previous node in the list (null if head).
    prev: AtomicPtr<CallbackNodeHeader>,
    /// Type-erased function to execute the callback.
    /// Takes pointer to this node header, which can be cast back to concrete type.
    execute: unsafe fn(*mut CallbackNodeHeader),
    /// Callback state.
    state: AtomicU8,
    /// Whether callback execution completed (for synchronization).
    executed: AtomicBool,
}

impl CallbackNodeHeader {
    /// Create a new node header with the given execute function.
    fn new(execute: unsafe fn(*mut CallbackNodeHeader)) -> Self {
        Self {
            next: AtomicPtr::new(std::ptr::null_mut()),
            prev: AtomicPtr::new(std::ptr::null_mut()),
            execute,
            state: AtomicU8::new(CB_NOT_REGISTERED),
            executed: AtomicBool::new(false),
        }
    }
}

/// Concrete callback node that contains the closure inline.
#[repr(C)]
struct CallbackNode<F> {
    /// Header must be first for safe casting.
    header: CallbackNodeHeader,
    /// The callback function stored inline.
    callback: UnsafeCell<Option<F>>,
}

/// An inplace stop source.
///
/// This is the primary stop source implementation. It supports:
/// - Thread-safe stop requests
/// - Callback registration and execution
/// - Proper synchronization for concurrent access
///
/// Callbacks are stored in an intrusive doubly-linked list, which avoids
/// the overhead of `Vec` reallocation when adding/removing callbacks.
#[derive(Debug)]
pub struct InplaceStopSource {
    inner: Arc<InplaceStopSourceInner>,
}

struct InplaceStopSourceInner {
    /// State flags (STOP_REQUESTED | LOCKED).
    state: AtomicU8,
    /// Head of callback list (protected by state LOCKED bit).
    callbacks_head: UnsafeCell<*mut CallbackNodeHeader>,
    /// Tail of callback list (protected by state LOCKED bit).
    callbacks_tail: UnsafeCell<*mut CallbackNodeHeader>,
    /// Thread that is currently notifying (for re-entrant callback deregistration).
    notifying_thread: Mutex<Option<thread::ThreadId>>,
}

// SAFETY: The inner state is accessed through atomic operations and mutex.
unsafe impl Send for InplaceStopSourceInner {}
unsafe impl Sync for InplaceStopSourceInner {}

impl std::fmt::Debug for InplaceStopSourceInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InplaceStopSourceInner")
            .field("state", &self.state)
            .field("notifying_thread", &self.notifying_thread)
            .finish()
    }
}

impl InplaceStopSourceInner {
    fn new() -> Self {
        Self {
            state: AtomicU8::new(0),
            callbacks_head: UnsafeCell::new(std::ptr::null_mut()),
            callbacks_tail: UnsafeCell::new(std::ptr::null_mut()),
            notifying_thread: Mutex::new(None),
        }
    }

    /// Acquire the lock (spin on LOCKED bit).
    fn lock(&self) -> u8 {
        loop {
            let old = self.state.load(Ordering::Relaxed);
            if old & LOCKED != 0 {
                std::hint::spin_loop();
                continue;
            }
            match self.state.compare_exchange_weak(
                old,
                old | LOCKED,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_) => return old,
                Err(_) => continue,
            }
        }
    }

    /// Release the lock.
    fn unlock(&self) {
        self.state.fetch_and(!LOCKED, Ordering::Release);
    }

    /// Add a callback node to the list (must hold lock).
    ///
    /// # Safety
    /// Caller must hold the lock and provide a valid node pointer.
    unsafe fn add_callback(&self, node: *mut CallbackNodeHeader) {
        // SAFETY: Caller guarantees we hold the lock.
        unsafe {
            let tail = *self.callbacks_tail.get();
            (*node).prev.store(tail, Ordering::Relaxed);
            (*node).next.store(std::ptr::null_mut(), Ordering::Relaxed);

            if tail.is_null() {
                // List was empty.
                *self.callbacks_head.get() = node;
            } else {
                (*tail).next.store(node, Ordering::Relaxed);
            }
            *self.callbacks_tail.get() = node;
        }
    }

    /// Remove a callback node from the list (must hold lock).
    ///
    /// # Safety
    /// Caller must hold the lock and provide a valid node pointer that is in the list.
    unsafe fn remove_callback(&self, node: *mut CallbackNodeHeader) {
        // SAFETY: Caller guarantees we hold the lock and node is in the list.
        unsafe {
            let prev = (*node).prev.load(Ordering::Relaxed);
            let next = (*node).next.load(Ordering::Relaxed);

            if prev.is_null() {
                // Node is head.
                *self.callbacks_head.get() = next;
            } else {
                (*prev).next.store(next, Ordering::Relaxed);
            }

            if next.is_null() {
                // Node is tail.
                *self.callbacks_tail.get() = prev;
            } else {
                (*next).prev.store(prev, Ordering::Relaxed);
            }
        }
    }
}

impl InplaceStopSource {
    /// Create a new stop source.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(InplaceStopSourceInner::new()),
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
        // Try to acquire the lock and set stop_requested.
        let mut old_state = self.inner.state.load(Ordering::Relaxed);
        loop {
            if old_state & STOP_REQUESTED != 0 {
                // Already stopped.
                return false;
            }

            // Spin while locked.
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

        // Record the notifying thread.
        *self.inner.notifying_thread.lock() = Some(thread::current().id());

        // Execute all callbacks.
        // SAFETY: We hold the lock.
        unsafe {
            let mut current = *self.inner.callbacks_head.get();
            while !current.is_null() {
                let next = (*current).next.load(Ordering::Relaxed);
                // Mark as executing.
                (*current).state.store(CB_EXECUTING, Ordering::Release);
                // Execute the callback.
                let execute = (*current).execute;
                execute(current);
                // Mark as done.
                (*current).state.store(CB_DONE, Ordering::Release);
                (*current).executed.store(true, Ordering::Release);
                current = next;
            }
            // Clear the list.
            *self.inner.callbacks_head.get() = std::ptr::null_mut();
            *self.inner.callbacks_tail.get() = std::ptr::null_mut();
        }

        // Release the lock but keep stop_requested.
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
    type Callback<F: FnOnce() + Send> = InplaceStopCallback<F>;

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
            // No source, callback will never be invoked.
            return InplaceStopCallback {
                node: None,
                source: None,
            };
        };

        // Check if stop was already requested (fast path).
        if inner.state.load(Ordering::Acquire) & STOP_REQUESTED != 0 {
            // Execute callback immediately.
            callback();
            return InplaceStopCallback {
                node: None,
                source: Some(inner.clone()),
            };
        }

        // Create the boxed callback node.
        let node = Box::new(CallbackNode {
            header: CallbackNodeHeader::new(execute_callback::<F>),
            callback: UnsafeCell::new(Some(callback)),
        });
        let node_ptr = Box::into_raw(node);

        // Acquire lock to register.
        let old_state = inner.lock();

        // Double-check stop wasn't requested while we were waiting for the lock.
        if old_state & STOP_REQUESTED != 0 {
            inner.unlock();
            // Take and execute the callback.
            // SAFETY: We just created the node and it's not in the list.
            let node = unsafe { Box::from_raw(node_ptr) };
            let f = unsafe { (*node.callback.get()).take() };
            if let Some(f) = f {
                f();
            }
            return InplaceStopCallback {
                node: None,
                source: Some(inner.clone()),
            };
        }

        // Register the callback.
        // SAFETY: We hold the lock, and node_ptr is valid.
        unsafe {
            (*node_ptr).header.state.store(CB_REGISTERED, Ordering::Release);
            inner.add_callback(node_ptr as *mut CallbackNodeHeader);
        }

        inner.unlock();

        InplaceStopCallback {
            node: Some(node_ptr),
            source: Some(inner.clone()),
        }
    }
}

/// Type-erased callback execution function.
///
/// # Safety
/// `header` must point to a valid `CallbackNode<F>`.
unsafe fn execute_callback<F: FnOnce() + Send>(header: *mut CallbackNodeHeader) {
    // SAFETY: header is the first field of CallbackNode due to #[repr(C)].
    let node = unsafe { &*(header as *const CallbackNode<F>) };

    // Take and execute the callback.
    // SAFETY: We're the only ones accessing the callback during execution.
    let f = unsafe { (*node.callback.get()).take() };
    if let Some(f) = f {
        f();
    }
}

/// Callback handle for inplace stop tokens.
///
/// This struct owns a heap-allocated callback node that contains the closure
/// inline. The node is part of an intrusive linked list in the stop source.
///
/// Dropping this handle deregisters the callback if it hasn't been invoked yet.
pub struct InplaceStopCallback<F: FnOnce() + Send> {
    /// Owned pointer to the callback node (None if already invoked or never registered).
    node: Option<*mut CallbackNode<F>>,
    /// Reference to the source (for deregistration).
    source: Option<Arc<InplaceStopSourceInner>>,
}

// SAFETY: The callback is Send because F is Send and we synchronize access.
unsafe impl<F: FnOnce() + Send> Send for InplaceStopCallback<F> {}

impl<F: FnOnce() + Send> std::fmt::Debug for InplaceStopCallback<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InplaceStopCallback")
            .field("has_node", &self.node.is_some())
            .finish()
    }
}

impl<F: FnOnce() + Send> StopCallbackHandle for InplaceStopCallback<F> {
    fn was_invoked(&self) -> bool {
        match self.node {
            None => {
                // No node means either never registered or already invoked.
                // If we have a source, it was invoked.
                self.source.is_some()
            }
            Some(node_ptr) => {
                // Check the node's executed flag.
                // SAFETY: node_ptr is valid if node is Some.
                unsafe { (*node_ptr).header.executed.load(Ordering::Acquire) }
            }
        }
    }
}

impl<F: FnOnce() + Send> Drop for InplaceStopCallback<F> {
    fn drop(&mut self) {
        let Some(node_ptr) = self.node.take() else {
            // No node to clean up.
            return;
        };

        let Some(source) = &self.source else {
            // No source, just free the node.
            // SAFETY: We own the node.
            unsafe {
                let _ = Box::from_raw(node_ptr);
            }
            return;
        };

        // SAFETY: node_ptr is valid.
        let state = unsafe { (*node_ptr).header.state.load(Ordering::Acquire) };

        // If not registered or already done, just free the node.
        if state == CB_NOT_REGISTERED || state == CB_DONE {
            // SAFETY: We own the node and it's not in the list.
            unsafe {
                let _ = Box::from_raw(node_ptr);
            }
            return;
        }

        // If stop was requested, we might be in the middle of execution.
        if source.state.load(Ordering::Acquire) & STOP_REQUESTED != 0 {
            // Check if we're on the notifying thread (re-entrant deregistration).
            let notifying_thread = source.notifying_thread.lock();
            if *notifying_thread == Some(thread::current().id()) {
                // We're being called from within callback execution.
                // The node is being iterated over, so we can't free it here.
                // Mark it for cleanup but don't free yet.
                // Actually, after iteration the list is cleared, so the node
                // will be orphaned. We still own it and should free it.
                drop(notifying_thread);
                // SAFETY: We own the node.
                unsafe {
                    let _ = Box::from_raw(node_ptr);
                }
                return;
            }
            drop(notifying_thread);

            // Wait for execution to complete if it's in progress.
            // SAFETY: node_ptr is valid.
            unsafe {
                while (*node_ptr).header.state.load(Ordering::Acquire) == CB_EXECUTING {
                    std::hint::spin_loop();
                }
            }

            // Now the node is done, free it.
            // SAFETY: We own the node and execution is complete.
            unsafe {
                let _ = Box::from_raw(node_ptr);
            }
            return;
        }

        // Acquire lock to deregister.
        source.lock();

        // Re-check state under lock.
        // SAFETY: node_ptr is valid.
        let state = unsafe { (*node_ptr).header.state.load(Ordering::Acquire) };
        if state == CB_REGISTERED {
            // SAFETY: We hold the lock and the callback is in the list.
            unsafe {
                source.remove_callback(node_ptr as *mut CallbackNodeHeader);
                (*node_ptr).header.state.store(CB_NOT_REGISTERED, Ordering::Release);
            }
        }

        source.unlock();

        // Free the node.
        // SAFETY: We own the node and it's no longer in the list.
        unsafe {
            let _ = Box::from_raw(node_ptr);
        }
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

    #[test]
    fn test_callback_deregistration() {
        use std::sync::atomic::AtomicUsize;

        let source = InplaceStopSource::new();
        let token = source.get_token();

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        // Register and then drop the callback before stop is requested.
        {
            let _callback = token.register_callback(move || {
                counter_clone.fetch_add(1, Ordering::SeqCst);
            });
        }

        // Now request stop - callback should NOT be invoked.
        source.request_stop();

        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }

    /// Test that stop token cancellation works through sender/receiver chain.
    #[test]
    fn test_stop_token_cancellation_through_chain() {
        use crate::traits::{
            Environment, OperationState, Receiver, ReceiverOf, ReceiverOfError, ReceiverOfStopped,
            Sender, SenderTo, ValueCompletion,
        };
        use std::pin::Pin;
        use std::sync::atomic::{AtomicBool, Ordering};

        // A sender that checks the stop token and cancels if requested
        struct CheckStopSender<T> {
            value: T,
        }

        impl<T: 'static> Sender for CheckStopSender<T> {
            type Completions = ValueCompletion<T>;
        }

        struct CheckStopOperation<T, R> {
            value: Option<T>,
            receiver: Option<R>,
        }

        impl<T, R> OperationState for CheckStopOperation<T, R>
        where
            R: ReceiverOf<T> + ReceiverOfStopped,
            R::Env: Environment,
            <R::Env as Environment>::StopToken: super::StopToken,
        {
            fn start(self: Pin<&mut Self>) {
                let this = unsafe { self.get_unchecked_mut() };
                if let (Some(value), Some(receiver)) = (this.value.take(), this.receiver.take()) {
                    // Check stop token from environment
                    let stop_token = receiver.get_env().get_stop_token();
                    if stop_token.stop_requested() {
                        receiver.set_stopped();
                    } else {
                        receiver.set_value(value);
                    }
                }
            }
        }

        impl<T: 'static, R> SenderTo<R> for CheckStopSender<T>
        where
            R: ReceiverOf<T> + ReceiverOfStopped,
            R::Env: Environment,
            <R::Env as Environment>::StopToken: super::StopToken,
        {
            type Operation = CheckStopOperation<T, R>;

            fn connect(self, receiver: R) -> Self::Operation {
                CheckStopOperation {
                    value: Some(self.value),
                    receiver: Some(receiver),
                }
            }
        }

        // Environment with our controllable stop token
        struct TestEnv {
            stop_token: InplaceStopToken,
        }

        impl Environment for TestEnv {
            type StopToken = InplaceStopToken;

            fn get_stop_token(&self) -> Self::StopToken {
                self.stop_token.clone()
            }
        }

        // Test receiver that tracks which completion was called
        struct TestReceiver {
            env: TestEnv,
            value_called: Arc<AtomicBool>,
            stopped_called: Arc<AtomicBool>,
            value_received: Arc<std::sync::Mutex<Option<i32>>>,
        }

        impl Receiver for TestReceiver {
            type Env = TestEnv;

            fn get_env(&self) -> &Self::Env {
                &self.env
            }
        }

        impl ReceiverOf<i32> for TestReceiver {
            fn set_value(self, value: i32) {
                *self.value_received.lock().unwrap() = Some(value);
                self.value_called.store(true, Ordering::SeqCst);
            }
        }

        impl ReceiverOfError<()> for TestReceiver {
            fn set_error(self, _: ()) {}
        }

        impl ReceiverOfStopped for TestReceiver {
            fn set_stopped(self) {
                self.stopped_called.store(true, Ordering::SeqCst);
            }
        }

        // Test 1: Without stop requested, value should pass through
        {
            let source = InplaceStopSource::new();
            let token = source.get_token();

            let value_called = Arc::new(AtomicBool::new(false));
            let stopped_called = Arc::new(AtomicBool::new(false));
            let value_received = Arc::new(std::sync::Mutex::new(None));

            let receiver = TestReceiver {
                env: TestEnv { stop_token: token },
                value_called: value_called.clone(),
                stopped_called: stopped_called.clone(),
                value_received: value_received.clone(),
            };

            let sender = CheckStopSender { value: 42 };
            let mut op = sender.connect(receiver);
            let pinned = unsafe { Pin::new_unchecked(&mut op) };
            pinned.start();

            assert!(
                value_called.load(Ordering::SeqCst),
                "set_value should be called"
            );
            assert!(
                !stopped_called.load(Ordering::SeqCst),
                "set_stopped should NOT be called"
            );
            assert_eq!(*value_received.lock().unwrap(), Some(42));
        }

        // Test 2: With stop requested, should complete with stopped
        {
            let source = InplaceStopSource::new();
            let token = source.get_token();

            // Request stop BEFORE starting
            source.request_stop();

            let value_called = Arc::new(AtomicBool::new(false));
            let stopped_called = Arc::new(AtomicBool::new(false));
            let value_received = Arc::new(std::sync::Mutex::new(None));

            let receiver = TestReceiver {
                env: TestEnv { stop_token: token },
                value_called: value_called.clone(),
                stopped_called: stopped_called.clone(),
                value_received: value_received.clone(),
            };

            let sender = CheckStopSender { value: 42 };
            let mut op = sender.connect(receiver);
            let pinned = unsafe { Pin::new_unchecked(&mut op) };
            pinned.start();

            assert!(
                !value_called.load(Ordering::SeqCst),
                "set_value should NOT be called"
            );
            assert!(
                stopped_called.load(Ordering::SeqCst),
                "set_stopped should be called"
            );
            assert_eq!(*value_received.lock().unwrap(), None);
        }
    }

    /// Test stop callback is invoked when stop is requested during operation
    #[test]
    fn test_stop_callback_during_operation() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let source = InplaceStopSource::new();
        let token = source.get_token();

        let callback_count = Arc::new(AtomicUsize::new(0));
        let callback_count_clone = callback_count.clone();

        // Register callback before stop is requested
        let _callback = token.register_callback(move || {
            callback_count_clone.fetch_add(1, Ordering::SeqCst);
        });

        assert_eq!(callback_count.load(Ordering::SeqCst), 0);

        // Request stop - callback should fire
        source.request_stop();

        assert_eq!(callback_count.load(Ordering::SeqCst), 1);

        // Requesting stop again should not fire callback again
        source.request_stop();
        assert_eq!(callback_count.load(Ordering::SeqCst), 1);
    }

    /// Test multiple callbacks are all invoked
    #[test]
    fn test_multiple_stop_callbacks() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let source = InplaceStopSource::new();
        let token = source.get_token();

        let counter1 = Arc::new(AtomicUsize::new(0));
        let counter2 = Arc::new(AtomicUsize::new(0));
        let counter3 = Arc::new(AtomicUsize::new(0));

        let c1 = counter1.clone();
        let c2 = counter2.clone();
        let c3 = counter3.clone();

        let _cb1 = token.register_callback(move || {
            c1.fetch_add(1, Ordering::SeqCst);
        });
        let _cb2 = token.register_callback(move || {
            c2.fetch_add(1, Ordering::SeqCst);
        });
        let _cb3 = token.register_callback(move || {
            c3.fetch_add(1, Ordering::SeqCst);
        });

        source.request_stop();

        assert_eq!(counter1.load(Ordering::SeqCst), 1);
        assert_eq!(counter2.load(Ordering::SeqCst), 1);
        assert_eq!(counter3.load(Ordering::SeqCst), 1);
    }

    /// Test partial deregistration - some callbacks dropped before stop
    #[test]
    fn test_partial_deregistration() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let source = InplaceStopSource::new();
        let token = source.get_token();

        let counter1 = Arc::new(AtomicUsize::new(0));
        let counter2 = Arc::new(AtomicUsize::new(0));
        let counter3 = Arc::new(AtomicUsize::new(0));

        let c1 = counter1.clone();
        let c2 = counter2.clone();
        let c3 = counter3.clone();

        let _cb1 = token.register_callback(move || {
            c1.fetch_add(1, Ordering::SeqCst);
        });

        // This one will be dropped before stop
        {
            let _cb2 = token.register_callback(move || {
                c2.fetch_add(1, Ordering::SeqCst);
            });
        }

        let _cb3 = token.register_callback(move || {
            c3.fetch_add(1, Ordering::SeqCst);
        });

        source.request_stop();

        assert_eq!(counter1.load(Ordering::SeqCst), 1);
        assert_eq!(counter2.load(Ordering::SeqCst), 0); // Was deregistered
        assert_eq!(counter3.load(Ordering::SeqCst), 1);
    }

    /// Test detached token
    #[test]
    fn test_detached_token() {
        let token = InplaceStopToken::new_detached();

        assert!(!token.stop_requested());
        assert!(!token.stop_possible());

        let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter_clone = counter.clone();

        // Callback on detached token should not be invoked
        let callback = token.register_callback(move || {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        });

        assert!(!callback.was_invoked());
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }
}
