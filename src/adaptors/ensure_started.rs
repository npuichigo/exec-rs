//! Ensure started adaptor - eagerly start a sender.
//!
//! `ensure_started(sender)` starts the sender immediately and returns a new
//! sender that completes with the result when connected. Unlike `split`,
//! this sender is move-only and can only be connected once.
//!
//! This implementation uses a single heap allocation containing both the
//! shared state and the underlying operation, with type-erased function
//! pointers for the waiter callback.

use crate::traits::{
    CompletionSignatures, Environment, OperationState, Receiver, ReceiverOf, ReceiverOfError,
    ReceiverOfStopped, Sender, SenderTo,
};
use std::cell::UnsafeCell;
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

/// State flags for ensure_started.
const STATE_RUNNING: u8 = 0;
const STATE_COMPLETED: u8 = 1;
const STATE_WAITING: u8 = 2;

/// Result stored in the shared state.
enum EnsureStartedResult<V, E> {
    Value(V),
    Error(E),
    Stopped,
}

/// Header containing shared state (accessible via raw pointer).
///
/// This is the first field of `EnsureStartedState<V, E, Op>` for safe casting.
#[repr(C)]
struct EnsureStartedHeader<V, E> {
    /// State: RUNNING, COMPLETED, or WAITING
    state: AtomicU8,
    /// The result (set by receiver, read by operation)
    result: UnsafeCell<Option<EnsureStartedResult<V, E>>>,
    /// Type-erased pointer to the waiter (EnsureStartedOperation)
    waiter_ptr: UnsafeCell<*mut ()>,
    /// Type-erased function to notify the waiter
    waiter_fn: UnsafeCell<Option<unsafe fn(*mut (), EnsureStartedResult<V, E>)>>,
}

impl<V, E> EnsureStartedHeader<V, E> {
    fn new() -> Self {
        Self {
            state: AtomicU8::new(STATE_RUNNING),
            result: UnsafeCell::new(None),
            waiter_ptr: UnsafeCell::new(std::ptr::null_mut()),
            waiter_fn: UnsafeCell::new(None),
        }
    }
}

/// Full state including the underlying operation.
#[repr(C)]
struct EnsureStartedState<V, E, Op> {
    /// Header must be first for safe pointer casting.
    header: EnsureStartedHeader<V, E>,
    /// The underlying operation (initialized after header).
    operation: MaybeUninit<Op>,
    /// Whether operation has been initialized.
    op_initialized: bool,
}

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
pub fn ensure_started<S, V, E>(sender: S) -> EnsureStarted<V, E>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>
        + SenderTo<EnsureStartedReceiver<V, E>>
        + Send
        + 'static,
    <S as SenderTo<EnsureStartedReceiver<V, E>>>::Operation: Send + 'static,
    V: Send + 'static,
    E: Send + 'static,
{
    // Allocate state with header initialized, operation uninitialized
    let state = Box::new(EnsureStartedState::<V, E, <S as SenderTo<EnsureStartedReceiver<V, E>>>::Operation> {
        header: EnsureStartedHeader::new(),
        operation: MaybeUninit::uninit(),
        op_initialized: false,
    });
    let state_ptr = Box::into_raw(state);

    // Create receiver pointing to header
    let receiver = EnsureStartedReceiver {
        header: state_ptr as *mut EnsureStartedHeader<V, E>,
        _marker: PhantomData,
    };

    // Connect to get operation
    let op = sender.connect(receiver);

    // Initialize operation in the state
    // SAFETY: state_ptr is valid, we just allocated it
    unsafe {
        (*state_ptr).operation.write(op);
        (*state_ptr).op_initialized = true;
    }

    // Start the operation
    // SAFETY: operation is initialized and state is heap-allocated (stable address)
    unsafe {
        let op_ptr = (*state_ptr).operation.assume_init_mut();
        Pin::new_unchecked(op_ptr).start();
    }

    EnsureStarted {
        header: state_ptr as *mut EnsureStartedHeader<V, E>,
        state_ptr: state_ptr as *mut (),
        drop_fn: drop_state::<V, E, <S as SenderTo<EnsureStartedReceiver<V, E>>>::Operation>,
        _marker: PhantomData,
    }
}

/// Drop function for the state.
unsafe fn drop_state<V, E, Op>(ptr: *mut ()) {
    let state = unsafe { Box::from_raw(ptr as *mut EnsureStartedState<V, E, Op>) };
    if state.op_initialized {
        // Drop the operation
        unsafe {
            std::ptr::drop_in_place(state.operation.as_ptr() as *mut Op);
        }
    }
    // Box::drop handles the rest
}

/// A sender that was eagerly started.
pub struct EnsureStarted<V, E> {
    /// Pointer to header (for accessing shared state).
    header: *mut EnsureStartedHeader<V, E>,
    /// Type-erased pointer to full state (for dropping).
    state_ptr: *mut (),
    /// Function to drop the state.
    drop_fn: unsafe fn(*mut ()),
    _marker: PhantomData<(V, E)>,
}

// SAFETY: The state is Send, and we synchronize access through atomics.
unsafe impl<V: Send, E: Send> Send for EnsureStarted<V, E> {}

impl<V, E> std::fmt::Debug for EnsureStarted<V, E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = unsafe { (*self.header).state.load(Ordering::Relaxed) };
        f.debug_struct("EnsureStarted")
            .field(
                "state",
                &match state {
                    STATE_RUNNING => "running",
                    STATE_COMPLETED => "completed",
                    STATE_WAITING => "waiting",
                    _ => "unknown",
                },
            )
            .finish()
    }
}

impl<V, E> Drop for EnsureStarted<V, E> {
    fn drop(&mut self) {
        // SAFETY: We own the state and drop_fn matches the state type.
        unsafe {
            (self.drop_fn)(self.state_ptr);
        }
    }
}

/// Completion signatures for ensure_started sender.
pub struct EnsureStartedCompletions<V, E> {
    _phantom: PhantomData<(V, E)>,
}

impl<V: 'static, E: 'static> CompletionSignatures for EnsureStartedCompletions<V, E> {
    type Value = V;
    type Error = E;
    const SENDS_STOPPED: bool = true;
}

impl<V: 'static, E: 'static> Sender for EnsureStarted<V, E> {
    type Completions = EnsureStartedCompletions<V, E>;
}

impl<V, E, R> SenderTo<R> for EnsureStarted<V, E>
where
    V: Send + 'static,
    E: Send + 'static,
    R: ReceiverOf<V> + ReceiverOfError<E> + ReceiverOfStopped + Send + 'static,
{
    type Operation = EnsureStartedOperation<V, E, R>;

    fn connect(self, receiver: R) -> Self::Operation {
        let header = self.header;
        let state_ptr = self.state_ptr;
        let drop_fn = self.drop_fn;

        // Prevent Drop from running (we're transferring ownership)
        std::mem::forget(self);

        EnsureStartedOperation {
            header,
            state_ptr,
            drop_fn,
            receiver: UnsafeCell::new(Some(receiver)),
            notified: AtomicBool::new(false),
            started: false,
            _marker: PhantomData,
        }
    }
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
pub struct EnsureStartedReceiver<V, E> {
    header: *mut EnsureStartedHeader<V, E>,
    _marker: PhantomData<(V, E)>,
}

// SAFETY: The header pointer is valid and we synchronize access.
unsafe impl<V: Send, E: Send> Send for EnsureStartedReceiver<V, E> {}

impl<V, E> Receiver for EnsureStartedReceiver<V, E> {
    type Env = EnsureStartedEnv;

    fn get_env(&self) -> &Self::Env {
        &EnsureStartedEnv
    }
}

impl<V: Send, E: Send> ReceiverOf<V> for EnsureStartedReceiver<V, E> {
    fn set_value(self, value: V) {
        self.complete(EnsureStartedResult::Value(value));
    }
}

impl<V: Send, E: Send> ReceiverOfError<E> for EnsureStartedReceiver<V, E> {
    fn set_error(self, error: E) {
        self.complete(EnsureStartedResult::Error(error));
    }
}

impl<V: Send, E: Send> ReceiverOfStopped for EnsureStartedReceiver<V, E> {
    fn set_stopped(self) {
        self.complete(EnsureStartedResult::Stopped);
    }
}

impl<V: Send, E: Send> EnsureStartedReceiver<V, E> {
    fn complete(self, result: EnsureStartedResult<V, E>) {
        // SAFETY: header is valid for the lifetime of the operation.
        let header = unsafe { &*self.header };

        // Try to transition from RUNNING to COMPLETED
        match header.state.compare_exchange(
            STATE_RUNNING,
            STATE_COMPLETED,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => {
                // We won - store the result
                unsafe {
                    *header.result.get() = Some(result);
                }
            }
            Err(STATE_WAITING) => {
                // A waiter is registered - notify it directly
                let waiter_ptr = unsafe { *header.waiter_ptr.get() };
                let waiter_fn = unsafe { (*header.waiter_fn.get()).take() };
                if let Some(notify) = waiter_fn {
                    // SAFETY: waiter_ptr and notify were set by the operation.
                    unsafe {
                        notify(waiter_ptr, result);
                    }
                }
            }
            Err(_) => {
                // Shouldn't happen (COMPLETED is terminal)
                unreachable!("ensure_started: unexpected state in complete");
            }
        }
    }
}

/// Operation state for ensure_started sender.
pub struct EnsureStartedOperation<V, E, R> {
    /// Pointer to header.
    header: *mut EnsureStartedHeader<V, E>,
    /// Type-erased pointer to full state.
    state_ptr: *mut (),
    /// Function to drop the state.
    drop_fn: unsafe fn(*mut ()),
    /// Downstream receiver.
    receiver: UnsafeCell<Option<R>>,
    /// Whether we've been notified.
    notified: AtomicBool,
    /// Whether start has been called.
    started: bool,
    _marker: PhantomData<(V, E)>,
}

// SAFETY: Fields are Send.
unsafe impl<V: Send, E: Send, R: Send> Send for EnsureStartedOperation<V, E, R> {}

impl<V, E, R> Drop for EnsureStartedOperation<V, E, R> {
    fn drop(&mut self) {
        // SAFETY: We own the state.
        unsafe {
            (self.drop_fn)(self.state_ptr);
        }
    }
}

/// Type-erased notification function.
///
/// # Safety
/// `op_ptr` must point to a valid `EnsureStartedOperation<V, E, R>`.
unsafe fn notify_operation<V, E, R>(op_ptr: *mut (), result: EnsureStartedResult<V, E>)
where
    R: ReceiverOf<V> + ReceiverOfError<E> + ReceiverOfStopped,
{
    let op = unsafe { &*(op_ptr as *const EnsureStartedOperation<V, E, R>) };

    // Only notify once
    if op.notified.swap(true, Ordering::AcqRel) {
        return;
    }

    // Take and notify the receiver
    let receiver = unsafe { (*op.receiver.get()).take() };
    if let Some(r) = receiver {
        match result {
            EnsureStartedResult::Value(v) => r.set_value(v),
            EnsureStartedResult::Error(e) => r.set_error(e),
            EnsureStartedResult::Stopped => r.set_stopped(),
        }
    }
}

impl<V, E, R> OperationState for EnsureStartedOperation<V, E, R>
where
    V: Send + 'static,
    E: Send + 'static,
    R: ReceiverOf<V> + ReceiverOfError<E> + ReceiverOfStopped + Send + 'static,
{
    fn start(self: Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };

        if this.started {
            return;
        }
        this.started = true;

        // SAFETY: header is valid.
        let header = unsafe { &*this.header };

        // Try to transition from RUNNING to WAITING (register ourselves as waiter)
        match header.state.compare_exchange(
            STATE_RUNNING,
            STATE_WAITING,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => {
                // We registered as waiter - store our info
                let self_ptr = this as *mut Self as *mut ();
                let notify_fn: unsafe fn(*mut (), EnsureStartedResult<V, E>) =
                    notify_operation::<V, E, R>;

                unsafe {
                    *header.waiter_ptr.get() = self_ptr;
                    *header.waiter_fn.get() = Some(notify_fn);
                }

                // The receiver will notify us when it completes.
            }
            Err(STATE_COMPLETED) => {
                // Already completed - get the result and notify immediately
                let result = unsafe { (*header.result.get()).take() };
                if let Some(result) = result {
                    if !this.notified.swap(true, Ordering::AcqRel) {
                        let receiver = unsafe { (*this.receiver.get()).take() };
                        if let Some(r) = receiver {
                            match result {
                                EnsureStartedResult::Value(v) => r.set_value(v),
                                EnsureStartedResult::Error(e) => r.set_error(e),
                                EnsureStartedResult::Stopped => r.set_stopped(),
                            }
                        }
                    }
                }
            }
            Err(_) => {
                // WAITING is invalid here (only one consumer allowed)
                unreachable!("ensure_started: unexpected state in start");
            }
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
    use std::sync::Arc;

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
