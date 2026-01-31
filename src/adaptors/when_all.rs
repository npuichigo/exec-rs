//! WhenAll adaptor - wait for multiple senders to complete.
//!
//! `when_all` runs multiple senders concurrently and collects all their results
//! into a tuple. If any sender fails or is stopped, the entire operation fails
//! or stops.
//!
//! This implementation is allocation-free, matching C++ stdexec's design.
//! The state lives inline in the operation and receivers hold raw pointers.

use crate::stop::{InplaceStopSource, InplaceStopToken, StopSource};
use crate::traits::{
    CompletionSignatures, Environment, OperationState, Receiver, ReceiverOf, ReceiverOfError,
    ReceiverOfStopped, Sender, SenderTo,
};
use std::cell::UnsafeCell;
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::pin::Pin;
use std::sync::atomic::{AtomicU8, Ordering};

/// State flags for when_all.
const STATE_STARTED: u8 = 0;
const STATE_ERROR: u8 = 1;
const STATE_STOPPED: u8 = 2;

/// Wait for two senders to complete and combine their results.
///
/// Both senders are started concurrently. When both complete successfully,
/// the results are combined into a tuple `(V1, V2)`.
///
/// If either sender completes with an error, the error is propagated and
/// the other sender is requested to stop.
///
/// If either sender is stopped, the stopped signal is propagated.
///
/// # Example
///
/// ```rust
/// use exec_rs::factories::just;
/// use exec_rs::adaptors::when_all;
/// use exec_rs::consumers::sync_wait;
///
/// let result = sync_wait(when_all(just(1), just(2))).unwrap();
/// assert_eq!(result, (1, 2));
/// ```
pub fn when_all<S1, S2>(sender1: S1, sender2: S2) -> WhenAll<S1, S2> {
    WhenAll::new(sender1, sender2)
}

/// A sender that waits for two senders to complete.
#[derive(Debug, Clone)]
pub struct WhenAll<S1, S2> {
    sender1: S1,
    sender2: S2,
}

impl<S1, S2> WhenAll<S1, S2> {
    /// Create a new WhenAll sender.
    pub fn new(sender1: S1, sender2: S2) -> Self {
        Self { sender1, sender2 }
    }
}

/// Completion signatures for WhenAll.
pub struct WhenAllCompletions<V1, V2, E>(PhantomData<(V1, V2, E)>);

impl<V1, V2, E> CompletionSignatures for WhenAllCompletions<V1, V2, E> {
    type Value = (V1, V2);
    type Error = E;
    const SENDS_STOPPED: bool = true;
}

impl<S1, S2, V1, V2, E> Sender for WhenAll<S1, S2>
where
    S1: Sender<Completions: CompletionSignatures<Value = V1, Error = E>>,
    S2: Sender<Completions: CompletionSignatures<Value = V2, Error = E>>,
{
    type Completions = WhenAllCompletions<V1, V2, E>;
}

impl<S1, S2, R, V1, V2, E> SenderTo<R> for WhenAll<S1, S2>
where
    S1: Sender<Completions: CompletionSignatures<Value = V1, Error = E>>
        + SenderTo<WhenAllReceiver1<V1, V2, E, R>>,
    S2: Sender<Completions: CompletionSignatures<Value = V2, Error = E>>
        + SenderTo<WhenAllReceiver2<V1, V2, E, R>>,
    R: ReceiverOf<(V1, V2)> + ReceiverOfError<E> + ReceiverOfStopped + Clone,
    V1: 'static,
    V2: 'static,
    E: 'static,
{
    type Operation = WhenAllOperation<S1, S2, R, V1, V2, E>;

    fn connect(self, receiver: R) -> Self::Operation {
        WhenAllOperation {
            // Store senders - they will be connected in start()
            sender1: Some(self.sender1),
            sender2: Some(self.sender2),
            // State is inline - no heap allocation!
            state: WhenAllState::new(receiver),
            // Operations are created in start() after we're pinned
            op1: UnsafeCell::new(MaybeUninit::uninit()),
            op2: UnsafeCell::new(MaybeUninit::uninit()),
            started: false,
        }
    }
}

/// Inline state for when_all operation - no heap allocation.
struct WhenAllState<V1, V2, E, R> {
    /// Atomic state: started, error, or stopped.
    state: AtomicU8,
    /// Counter of remaining senders (starts at 2).
    count: AtomicU8,
    /// Stop source for cancelling child operations.
    stop_source: InplaceStopSource,
    /// Storage for first sender's value.
    value1: UnsafeCell<Option<V1>>,
    /// Storage for second sender's value.
    value2: UnsafeCell<Option<V2>>,
    /// Storage for error.
    error: UnsafeCell<Option<E>>,
    /// The downstream receiver.
    receiver: UnsafeCell<Option<R>>,
}

// SAFETY: Access to UnsafeCell fields is synchronized via atomic operations.
// The receiver is only accessed when count reaches 0.
unsafe impl<V1: Send, V2: Send, E: Send, R: Send> Send for WhenAllState<V1, V2, E, R> {}
unsafe impl<V1: Send, V2: Send, E: Send, R: Send> Sync for WhenAllState<V1, V2, E, R> {}

impl<V1, V2, E, R> WhenAllState<V1, V2, E, R>
where
    R: ReceiverOf<(V1, V2)> + ReceiverOfError<E> + ReceiverOfStopped,
{
    fn new(receiver: R) -> Self {
        Self {
            state: AtomicU8::new(STATE_STARTED),
            count: AtomicU8::new(2),
            stop_source: InplaceStopSource::new(),
            value1: UnsafeCell::new(None),
            value2: UnsafeCell::new(None),
            error: UnsafeCell::new(None),
            receiver: UnsafeCell::new(Some(receiver)),
        }
    }

    /// Called when a child operation completes. Decrements counter and
    /// completes if this is the last one.
    ///
    /// SAFETY: Called via raw pointer from receivers. The pointer is valid
    /// because the WhenAllOperation is pinned and outlives all receivers.
    unsafe fn arrive(&self) {
        if self.count.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.complete();
        }
    }

    /// Complete the when_all operation based on final state.
    fn complete(&self) {
        // SAFETY: We're the only thread accessing these after count reaches 0
        let receiver = unsafe { (*self.receiver.get()).take() };
        let Some(receiver) = receiver else { return };

        match self.state.load(Ordering::Acquire) {
            STATE_STARTED => {
                // Both completed successfully
                let v1 = unsafe { (*self.value1.get()).take() };
                let v2 = unsafe { (*self.value2.get()).take() };
                if let (Some(v1), Some(v2)) = (v1, v2) {
                    receiver.set_value((v1, v2));
                }
            }
            STATE_ERROR => {
                // One completed with error
                let error = unsafe { (*self.error.get()).take() };
                if let Some(error) = error {
                    receiver.set_error(error);
                }
            }
            STATE_STOPPED => {
                // One was stopped
                receiver.set_stopped();
            }
            _ => unreachable!(),
        }
    }

    /// Transition to error state and save the error.
    fn set_error(&self, error: E) {
        let prev = self.state.swap(STATE_ERROR, Ordering::AcqRel);
        if prev == STATE_STARTED {
            // We're the first to fail, save the error and request stop
            unsafe {
                *self.error.get() = Some(error);
            }
            self.stop_source.request_stop();
        }
        // If already in error or stopped state, ignore this error
    }

    /// Transition to stopped state.
    fn set_stopped(&self) {
        let prev = self.state.swap(STATE_STOPPED, Ordering::AcqRel);
        if prev == STATE_STARTED {
            self.stop_source.request_stop();
        }
    }
}

/// Environment for when_all receivers.
pub struct WhenAllEnv {
    stop_token: InplaceStopToken,
}

impl Environment for WhenAllEnv {
    type StopToken = InplaceStopToken;

    fn get_stop_token(&self) -> Self::StopToken {
        self.stop_token.clone()
    }
}

/// Receiver for the first sender in when_all.
///
/// Holds a raw pointer to the inline state. This is safe because:
/// 1. The WhenAllOperation is pinned when start() is called
/// 2. The receiver is consumed when completion signals are delivered
/// 3. The operation outlives all child operations
pub struct WhenAllReceiver1<V1, V2, E, R> {
    state: *const WhenAllState<V1, V2, E, R>,
}

// SAFETY: The pointer points to a WhenAllState that is Send+Sync
unsafe impl<V1: Send, V2: Send, E: Send, R: Send> Send for WhenAllReceiver1<V1, V2, E, R> {}
unsafe impl<V1: Send, V2: Send, E: Send, R: Send> Sync for WhenAllReceiver1<V1, V2, E, R> {}

impl<V1, V2, E, R> Receiver for WhenAllReceiver1<V1, V2, E, R>
where
    R: ReceiverOf<(V1, V2)> + ReceiverOfError<E> + ReceiverOfStopped,
{
    type Env = WhenAllEnv;

    fn get_env(&self) -> &Self::Env {
        thread_local! {
            static ENV: WhenAllEnv = WhenAllEnv {
                stop_token: InplaceStopToken::new_detached(),
            };
        }
        ENV.with(|env| unsafe { &*(env as *const WhenAllEnv) })
    }
}

impl<V1, V2, E, R> ReceiverOf<V1> for WhenAllReceiver1<V1, V2, E, R>
where
    R: ReceiverOf<(V1, V2)> + ReceiverOfError<E> + ReceiverOfStopped,
{
    fn set_value(self, value: V1) {
        // SAFETY: Pointer is valid because operation is pinned
        let state = unsafe { &*self.state };
        unsafe {
            *state.value1.get() = Some(value);
            state.arrive();
        }
    }
}

impl<V1, V2, E, R> ReceiverOfError<E> for WhenAllReceiver1<V1, V2, E, R>
where
    R: ReceiverOf<(V1, V2)> + ReceiverOfError<E> + ReceiverOfStopped,
{
    fn set_error(self, error: E) {
        let state = unsafe { &*self.state };
        state.set_error(error);
        unsafe { state.arrive() };
    }
}

impl<V1, V2, E, R> ReceiverOfStopped for WhenAllReceiver1<V1, V2, E, R>
where
    R: ReceiverOf<(V1, V2)> + ReceiverOfError<E> + ReceiverOfStopped,
{
    fn set_stopped(self) {
        let state = unsafe { &*self.state };
        state.set_stopped();
        unsafe { state.arrive() };
    }
}

/// Receiver for the second sender in when_all.
pub struct WhenAllReceiver2<V1, V2, E, R> {
    state: *const WhenAllState<V1, V2, E, R>,
}

unsafe impl<V1: Send, V2: Send, E: Send, R: Send> Send for WhenAllReceiver2<V1, V2, E, R> {}
unsafe impl<V1: Send, V2: Send, E: Send, R: Send> Sync for WhenAllReceiver2<V1, V2, E, R> {}

impl<V1, V2, E, R> Receiver for WhenAllReceiver2<V1, V2, E, R>
where
    R: ReceiverOf<(V1, V2)> + ReceiverOfError<E> + ReceiverOfStopped,
{
    type Env = WhenAllEnv;

    fn get_env(&self) -> &Self::Env {
        thread_local! {
            static ENV: WhenAllEnv = WhenAllEnv {
                stop_token: InplaceStopToken::new_detached(),
            };
        }
        ENV.with(|env| unsafe { &*(env as *const WhenAllEnv) })
    }
}

impl<V1, V2, E, R> ReceiverOf<V2> for WhenAllReceiver2<V1, V2, E, R>
where
    R: ReceiverOf<(V1, V2)> + ReceiverOfError<E> + ReceiverOfStopped,
{
    fn set_value(self, value: V2) {
        let state = unsafe { &*self.state };
        unsafe {
            *state.value2.get() = Some(value);
            state.arrive();
        }
    }
}

impl<V1, V2, E, R> ReceiverOfError<E> for WhenAllReceiver2<V1, V2, E, R>
where
    R: ReceiverOf<(V1, V2)> + ReceiverOfError<E> + ReceiverOfStopped,
{
    fn set_error(self, error: E) {
        let state = unsafe { &*self.state };
        state.set_error(error);
        unsafe { state.arrive() };
    }
}

impl<V1, V2, E, R> ReceiverOfStopped for WhenAllReceiver2<V1, V2, E, R>
where
    R: ReceiverOf<(V1, V2)> + ReceiverOfError<E> + ReceiverOfStopped,
{
    fn set_stopped(self) {
        let state = unsafe { &*self.state };
        state.set_stopped();
        unsafe { state.arrive() };
    }
}

/// Operation state for WhenAll - allocation-free.
///
/// The state is stored inline and receivers hold raw pointers to it.
/// This is safe because the operation is pinned when start() is called.
pub struct WhenAllOperation<S1, S2, R, V1, V2, E>
where
    S1: SenderTo<WhenAllReceiver1<V1, V2, E, R>>,
    S2: SenderTo<WhenAllReceiver2<V1, V2, E, R>>,
    R: ReceiverOf<(V1, V2)> + ReceiverOfError<E> + ReceiverOfStopped,
{
    /// Senders stored until start() - then moved out
    sender1: Option<S1>,
    sender2: Option<S2>,
    /// Inline state - no heap allocation
    state: WhenAllState<V1, V2, E, R>,
    /// Child operations - initialized in start() after we're pinned
    op1: UnsafeCell<MaybeUninit<S1::Operation>>,
    op2: UnsafeCell<MaybeUninit<S2::Operation>>,
    started: bool,
}

// SAFETY: The operations are only accessed from the thread that calls start().
unsafe impl<S1, S2, R, V1, V2, E> Send for WhenAllOperation<S1, S2, R, V1, V2, E>
where
    S1: SenderTo<WhenAllReceiver1<V1, V2, E, R>> + Send,
    S2: SenderTo<WhenAllReceiver2<V1, V2, E, R>> + Send,
    R: ReceiverOf<(V1, V2)> + ReceiverOfError<E> + ReceiverOfStopped + Send,
    V1: Send,
    V2: Send,
    E: Send,
    S1::Operation: Send,
    S2::Operation: Send,
{
}

impl<S1, S2, R, V1, V2, E> OperationState for WhenAllOperation<S1, S2, R, V1, V2, E>
where
    S1: SenderTo<WhenAllReceiver1<V1, V2, E, R>>,
    S2: SenderTo<WhenAllReceiver2<V1, V2, E, R>>,
    R: ReceiverOf<(V1, V2)> + ReceiverOfError<E> + ReceiverOfStopped,
{
    fn start(self: Pin<&mut Self>) {
        // SAFETY: We don't move self, only access fields
        let this = unsafe { self.get_unchecked_mut() };

        if this.started {
            return;
        }
        this.started = true;

        // Take the senders - they're consumed here
        let sender1 = this.sender1.take().expect("sender1 already taken");
        let sender2 = this.sender2.take().expect("sender2 already taken");

        // Create receivers with raw pointers to our inline state.
        // SAFETY: The operation is now pinned, so the state pointer is stable.
        let state_ptr = &this.state as *const WhenAllState<V1, V2, E, R>;

        let receiver1 = WhenAllReceiver1 { state: state_ptr };
        let receiver2 = WhenAllReceiver2 { state: state_ptr };

        // Connect senders to receivers and store operations
        let op1 = sender1.connect(receiver1);
        let op2 = sender2.connect(receiver2);

        unsafe {
            (*this.op1.get()).write(op1);
            (*this.op2.get()).write(op2);
        }

        // Start both operations
        unsafe {
            let op1_ptr = (*this.op1.get()).as_mut_ptr();
            let op2_ptr = (*this.op2.get()).as_mut_ptr();

            Pin::new_unchecked(&mut *op1_ptr).start();
            Pin::new_unchecked(&mut *op2_ptr).start();
        }
    }
}

impl<S1, S2, R, V1, V2, E> Drop for WhenAllOperation<S1, S2, R, V1, V2, E>
where
    S1: SenderTo<WhenAllReceiver1<V1, V2, E, R>>,
    S2: SenderTo<WhenAllReceiver2<V1, V2, E, R>>,
    R: ReceiverOf<(V1, V2)> + ReceiverOfError<E> + ReceiverOfStopped,
{
    fn drop(&mut self) {
        // Only drop the operations if they were initialized (i.e., if start() was called)
        if self.started {
            unsafe {
                std::ptr::drop_in_place((*self.op1.get()).as_mut_ptr());
                std::ptr::drop_in_place((*self.op2.get()).as_mut_ptr());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adaptors::then;
    use crate::consumers::sync_wait;
    use crate::factories::{just, just_error, schedule};
    use crate::schedulers::InlineScheduler;
    use crate::traits::Sender;

    // ========================================================================
    // test_when_all.cpp equivalents
    // ========================================================================

    #[test]
    fn test_when_all_basic() {
        let result = sync_wait(when_all(just(1), just(2))).unwrap();
        assert_eq!(result, (1, 2));
    }

    #[test]
    fn test_when_all_different_types() {
        let result = sync_wait(when_all(just(42), just("hello"))).unwrap();
        assert_eq!(result, (42, "hello"));
    }

    #[test]
    fn test_when_all_strings() {
        let result = sync_wait(when_all(
            just("first".to_string()),
            just("second".to_string()),
        ))
        .unwrap();
        assert_eq!(result, ("first".to_string(), "second".to_string()));
    }

    #[test]
    fn test_when_all_unit() {
        let result = sync_wait(when_all(just(()), just(()))).unwrap();
        assert_eq!(result, ((), ()));
    }

    #[test]
    fn test_when_all_is_sender() {
        fn assert_sender<S: Sender>(_: &S) {}
        let s = when_all(just(1), just(2));
        assert_sender(&s);
    }

    #[test]
    fn test_when_all_sum_values() {
        let (a, b) = sync_wait(when_all(just(10), just(20))).unwrap();
        assert_eq!(a + b, 30);
    }

    #[test]
    fn test_when_all_first_error() {
        // Both senders must have the same error type
        let result = sync_wait(when_all(
            just_error::<_>("error1"),
            just_error::<_>("error2"),
        ));
        assert!(result.is_err());
    }

    #[test]
    fn test_when_all_both_errors() {
        // Both complete with errors
        let result = sync_wait(when_all(
            just_error::<_>("first"),
            just_error::<_>("second"),
        ));
        assert!(result.is_err());
    }

    #[test]
    fn test_when_all_with_schedule() {
        let scheduler = InlineScheduler;
        let result = sync_wait(when_all(
            then(schedule(scheduler), |_| 1),
            then(schedule(scheduler), |_| 2),
        ))
        .unwrap();
        assert_eq!(result, (1, 2));
    }


    #[test]
    fn test_when_all_move_only_values() {
        let b1 = Box::new(1);
        let b2 = Box::new(2);
        let result = sync_wait(when_all(just(b1), just(b2))).unwrap();
        assert_eq!(*result.0, 1);
        assert_eq!(*result.1, 2);
    }

    #[test]
    fn test_when_all_with_transformation() {
        let result = sync_wait(when_all(
            then(just(10), |x| x * 2),
            then(just(5), |x| x * 3),
        ))
        .unwrap();
        assert_eq!(result, (20, 15));
    }
}
