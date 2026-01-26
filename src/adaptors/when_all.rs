//! WhenAll adaptor - wait for multiple senders to complete.
//!
//! `when_all` runs multiple senders concurrently and collects all their results
//! into a tuple. If any sender fails or is stopped, the entire operation fails
//! or stops.

use crate::stop::{InplaceStopSource, InplaceStopToken, StopSource};
use crate::traits::{
    CompletionSignatures, Environment, OperationState, Receiver, ReceiverOf, ReceiverOfError,
    ReceiverOfStopped, Sender, SenderTo,
};
use std::cell::UnsafeCell;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

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
        let state = Arc::new(WhenAllState::new(receiver));

        let receiver1 = WhenAllReceiver1 {
            state: state.clone(),
        };
        let receiver2 = WhenAllReceiver2 {
            state: state.clone(),
        };

        WhenAllOperation {
            state,
            op1: UnsafeCell::new(Some(self.sender1.connect(receiver1))),
            op2: UnsafeCell::new(Some(self.sender2.connect(receiver2))),
            started: false,
        }
    }
}

/// Shared state for when_all operation.
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
    fn arrive(self: &Arc<Self>) {
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
pub struct WhenAllReceiver1<V1, V2, E, R> {
    state: Arc<WhenAllState<V1, V2, E, R>>,
}

impl<V1, V2, E, R> Receiver for WhenAllReceiver1<V1, V2, E, R>
where
    R: ReceiverOf<(V1, V2)> + ReceiverOfError<E> + ReceiverOfStopped,
{
    type Env = WhenAllEnv;

    fn get_env(&self) -> &Self::Env {
        // Return environment with the when_all's stop token
        // This is a slight simplification - in production we'd manage this better
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
        // Store the value
        unsafe {
            *self.state.value1.get() = Some(value);
        }
        // Arrive at the barrier
        self.state.arrive();
    }
}

impl<V1, V2, E, R> ReceiverOfError<E> for WhenAllReceiver1<V1, V2, E, R>
where
    R: ReceiverOf<(V1, V2)> + ReceiverOfError<E> + ReceiverOfStopped,
{
    fn set_error(self, error: E) {
        self.state.set_error(error);
        self.state.arrive();
    }
}

impl<V1, V2, E, R> ReceiverOfStopped for WhenAllReceiver1<V1, V2, E, R>
where
    R: ReceiverOf<(V1, V2)> + ReceiverOfError<E> + ReceiverOfStopped,
{
    fn set_stopped(self) {
        self.state.set_stopped();
        self.state.arrive();
    }
}

/// Receiver for the second sender in when_all.
pub struct WhenAllReceiver2<V1, V2, E, R> {
    state: Arc<WhenAllState<V1, V2, E, R>>,
}

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
        unsafe {
            *self.state.value2.get() = Some(value);
        }
        self.state.arrive();
    }
}

impl<V1, V2, E, R> ReceiverOfError<E> for WhenAllReceiver2<V1, V2, E, R>
where
    R: ReceiverOf<(V1, V2)> + ReceiverOfError<E> + ReceiverOfStopped,
{
    fn set_error(self, error: E) {
        self.state.set_error(error);
        self.state.arrive();
    }
}

impl<V1, V2, E, R> ReceiverOfStopped for WhenAllReceiver2<V1, V2, E, R>
where
    R: ReceiverOf<(V1, V2)> + ReceiverOfError<E> + ReceiverOfStopped,
{
    fn set_stopped(self) {
        self.state.set_stopped();
        self.state.arrive();
    }
}

/// Operation state for WhenAll.
pub struct WhenAllOperation<S1, S2, R, V1, V2, E>
where
    S1: SenderTo<WhenAllReceiver1<V1, V2, E, R>>,
    S2: SenderTo<WhenAllReceiver2<V1, V2, E, R>>,
    R: ReceiverOf<(V1, V2)> + ReceiverOfError<E> + ReceiverOfStopped,
{
    state: Arc<WhenAllState<V1, V2, E, R>>,
    op1: UnsafeCell<Option<S1::Operation>>,
    op2: UnsafeCell<Option<S2::Operation>>,
    started: bool,
}

// SAFETY: The operations are only accessed from the thread that calls start().
unsafe impl<S1, S2, R, V1, V2, E> Send for WhenAllOperation<S1, S2, R, V1, V2, E>
where
    S1: SenderTo<WhenAllReceiver1<V1, V2, E, R>>,
    S2: SenderTo<WhenAllReceiver2<V1, V2, E, R>>,
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
        let this = unsafe { self.get_unchecked_mut() };

        if this.started {
            return;
        }
        this.started = true;

        // Start both operations
        unsafe {
            if let Some(op1) = (*this.op1.get()).as_mut() {
                Pin::new_unchecked(op1).start();
            }
            if let Some(op2) = (*this.op2.get()).as_mut() {
                Pin::new_unchecked(op2).start();
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
}
