//! WhenAny adaptor - race multiple senders, first to complete wins.
//!
//! `when_any` runs multiple senders concurrently. The first one to complete
//! determines the result, and the others are cancelled.

use crate::stop::{InplaceStopSource, InplaceStopToken, StopSource, StopToken};
use crate::traits::{
    CompletionSignatures, Environment, OperationState, Receiver, ReceiverOf, ReceiverOfError,
    ReceiverOfStopped, Sender, SenderTo,
};
use std::cell::UnsafeCell;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Race two senders, the first to complete wins.
///
/// Both senders are started concurrently. The first one to complete
/// (with value, error, or stopped) determines the result. The other
/// sender is requested to stop.
///
/// Note: Both senders must produce the same value and error types.
///
/// # Example
///
/// ```rust
/// use exec_rs::factories::just;
/// use exec_rs::adaptors::when_any;
/// use exec_rs::consumers::sync_wait;
///
/// // Both complete immediately, one will win (deterministic for just())
/// let result = sync_wait(when_any(just(1), just(2))).unwrap();
/// // Result is either 1 or 2
/// assert!(result == 1 || result == 2);
/// ```
pub fn when_any<S1, S2>(sender1: S1, sender2: S2) -> WhenAny<S1, S2> {
    WhenAny::new(sender1, sender2)
}

/// A sender that races two senders.
#[derive(Debug, Clone)]
pub struct WhenAny<S1, S2> {
    sender1: S1,
    sender2: S2,
}

impl<S1, S2> WhenAny<S1, S2> {
    /// Create a new WhenAny sender.
    pub fn new(sender1: S1, sender2: S2) -> Self {
        Self { sender1, sender2 }
    }
}

/// Completion signatures for WhenAny.
/// Both senders must have the same value and error types.
pub struct WhenAnyCompletions<V, E>(PhantomData<(V, E)>);

impl<V, E> CompletionSignatures for WhenAnyCompletions<V, E> {
    type Value = V;
    type Error = E;
    const SENDS_STOPPED: bool = true;
}

impl<S1, S2, V, E> Sender for WhenAny<S1, S2>
where
    S1: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
    S2: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
{
    type Completions = WhenAnyCompletions<V, E>;
}

impl<S1, S2, R, V, E> SenderTo<R> for WhenAny<S1, S2>
where
    S1: Sender<Completions: CompletionSignatures<Value = V, Error = E>>
        + SenderTo<WhenAnyReceiver1<V, E, R>>,
    S2: Sender<Completions: CompletionSignatures<Value = V, Error = E>>
        + SenderTo<WhenAnyReceiver2<V, E, R>>,
    R: ReceiverOf<V> + ReceiverOfError<E> + ReceiverOfStopped + Clone,
    V: 'static,
    E: 'static,
{
    type Operation = WhenAnyOperation<S1, S2, R, V, E>;

    fn connect(self, receiver: R) -> Self::Operation {
        let state = Arc::new(WhenAnyState::new(receiver));

        let receiver1 = WhenAnyReceiver1 {
            state: state.clone(),
        };
        let receiver2 = WhenAnyReceiver2 {
            state: state.clone(),
        };

        WhenAnyOperation {
            state,
            op1: UnsafeCell::new(Some(self.sender1.connect(receiver1))),
            op2: UnsafeCell::new(Some(self.sender2.connect(receiver2))),
            started: false,
        }
    }
}

/// Shared state for when_any operation.
struct WhenAnyState<V, E, R> {
    /// Has a sender already completed?
    completed: AtomicBool,
    /// Counter of remaining senders that need to arrive (for cleanup).
    count: std::sync::atomic::AtomicU8,
    /// Stop source for cancelling the other sender.
    stop_source: InplaceStopSource,
    /// The downstream receiver.
    receiver: UnsafeCell<Option<R>>,
    /// Phantom data for value and error types.
    _phantom: PhantomData<(V, E)>,
}

// SAFETY: Access to UnsafeCell fields is synchronized via atomic operations.
unsafe impl<V: Send, E: Send, R: Send> Send for WhenAnyState<V, E, R> {}
unsafe impl<V: Send, E: Send, R: Send> Sync for WhenAnyState<V, E, R> {}

impl<V, E, R> WhenAnyState<V, E, R>
where
    R: ReceiverOf<V> + ReceiverOfError<E> + ReceiverOfStopped,
{
    fn new(receiver: R) -> Self {
        Self {
            completed: AtomicBool::new(false),
            count: std::sync::atomic::AtomicU8::new(2),
            stop_source: InplaceStopSource::new(),
            receiver: UnsafeCell::new(Some(receiver)),
            _phantom: PhantomData,
        }
    }

    /// Try to be the winner. Returns true if this is the first to complete.
    fn try_complete(&self) -> bool {
        // Try to be the first to set completed to true
        !self.completed.swap(true, Ordering::AcqRel)
    }

    /// Request stop for the other sender.
    fn request_stop(&self) {
        self.stop_source.request_stop();
    }

    /// Get the receiver (only valid for the winner).
    fn take_receiver(&self) -> Option<R> {
        unsafe { (*self.receiver.get()).take() }
    }

    /// Arrive at the barrier (for cleanup counting).
    fn arrive(&self) {
        self.count.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Environment for when_any receivers.
pub struct WhenAnyEnv {
    stop_token: InplaceStopToken,
}

impl Environment for WhenAnyEnv {
    type StopToken = InplaceStopToken;

    fn get_stop_token(&self) -> Self::StopToken {
        self.stop_token.clone()
    }
}

/// Receiver for the first sender in when_any.
pub struct WhenAnyReceiver1<V, E, R> {
    state: Arc<WhenAnyState<V, E, R>>,
}

impl<V, E, R> Receiver for WhenAnyReceiver1<V, E, R>
where
    R: ReceiverOf<V> + ReceiverOfError<E> + ReceiverOfStopped,
{
    type Env = WhenAnyEnv;

    fn get_env(&self) -> &Self::Env {
        thread_local! {
            static ENV: WhenAnyEnv = WhenAnyEnv {
                stop_token: InplaceStopToken::new_detached(),
            };
        }
        ENV.with(|env| unsafe { &*(env as *const WhenAnyEnv) })
    }
}

impl<V, E, R> ReceiverOf<V> for WhenAnyReceiver1<V, E, R>
where
    R: ReceiverOf<V> + ReceiverOfError<E> + ReceiverOfStopped,
{
    fn set_value(self, value: V) {
        if self.state.try_complete() {
            // We won the race!
            self.state.request_stop();
            if let Some(receiver) = self.state.take_receiver() {
                receiver.set_value(value);
            }
        }
        self.state.arrive();
    }
}

impl<V, E, R> ReceiverOfError<E> for WhenAnyReceiver1<V, E, R>
where
    R: ReceiverOf<V> + ReceiverOfError<E> + ReceiverOfStopped,
{
    fn set_error(self, error: E) {
        if self.state.try_complete() {
            self.state.request_stop();
            if let Some(receiver) = self.state.take_receiver() {
                receiver.set_error(error);
            }
        }
        self.state.arrive();
    }
}

impl<V, E, R> ReceiverOfStopped for WhenAnyReceiver1<V, E, R>
where
    R: ReceiverOf<V> + ReceiverOfError<E> + ReceiverOfStopped,
{
    fn set_stopped(self) {
        if self.state.try_complete() {
            self.state.request_stop();
            if let Some(receiver) = self.state.take_receiver() {
                receiver.set_stopped();
            }
        }
        self.state.arrive();
    }
}

/// Receiver for the second sender in when_any.
pub struct WhenAnyReceiver2<V, E, R> {
    state: Arc<WhenAnyState<V, E, R>>,
}

impl<V, E, R> Receiver for WhenAnyReceiver2<V, E, R>
where
    R: ReceiverOf<V> + ReceiverOfError<E> + ReceiverOfStopped,
{
    type Env = WhenAnyEnv;

    fn get_env(&self) -> &Self::Env {
        thread_local! {
            static ENV: WhenAnyEnv = WhenAnyEnv {
                stop_token: InplaceStopToken::new_detached(),
            };
        }
        ENV.with(|env| unsafe { &*(env as *const WhenAnyEnv) })
    }
}

impl<V, E, R> ReceiverOf<V> for WhenAnyReceiver2<V, E, R>
where
    R: ReceiverOf<V> + ReceiverOfError<E> + ReceiverOfStopped,
{
    fn set_value(self, value: V) {
        if self.state.try_complete() {
            self.state.request_stop();
            if let Some(receiver) = self.state.take_receiver() {
                receiver.set_value(value);
            }
        }
        self.state.arrive();
    }
}

impl<V, E, R> ReceiverOfError<E> for WhenAnyReceiver2<V, E, R>
where
    R: ReceiverOf<V> + ReceiverOfError<E> + ReceiverOfStopped,
{
    fn set_error(self, error: E) {
        if self.state.try_complete() {
            self.state.request_stop();
            if let Some(receiver) = self.state.take_receiver() {
                receiver.set_error(error);
            }
        }
        self.state.arrive();
    }
}

impl<V, E, R> ReceiverOfStopped for WhenAnyReceiver2<V, E, R>
where
    R: ReceiverOf<V> + ReceiverOfError<E> + ReceiverOfStopped,
{
    fn set_stopped(self) {
        if self.state.try_complete() {
            self.state.request_stop();
            if let Some(receiver) = self.state.take_receiver() {
                receiver.set_stopped();
            }
        }
        self.state.arrive();
    }
}

/// Operation state for WhenAny.
pub struct WhenAnyOperation<S1, S2, R, V, E>
where
    S1: SenderTo<WhenAnyReceiver1<V, E, R>>,
    S2: SenderTo<WhenAnyReceiver2<V, E, R>>,
    R: ReceiverOf<V> + ReceiverOfError<E> + ReceiverOfStopped,
{
    #[allow(dead_code)]
    state: Arc<WhenAnyState<V, E, R>>,
    op1: UnsafeCell<Option<S1::Operation>>,
    op2: UnsafeCell<Option<S2::Operation>>,
    started: bool,
}

// SAFETY: The operations are only accessed from the thread that calls start().
unsafe impl<S1, S2, R, V, E> Send for WhenAnyOperation<S1, S2, R, V, E>
where
    S1: SenderTo<WhenAnyReceiver1<V, E, R>>,
    S2: SenderTo<WhenAnyReceiver2<V, E, R>>,
    R: ReceiverOf<V> + ReceiverOfError<E> + ReceiverOfStopped + Send,
    V: Send,
    E: Send,
    S1::Operation: Send,
    S2::Operation: Send,
{
}

impl<S1, S2, R, V, E> OperationState for WhenAnyOperation<S1, S2, R, V, E>
where
    S1: SenderTo<WhenAnyReceiver1<V, E, R>>,
    S2: SenderTo<WhenAnyReceiver2<V, E, R>>,
    R: ReceiverOf<V> + ReceiverOfError<E> + ReceiverOfStopped,
{
    fn start(self: Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };

        if this.started {
            return;
        }
        this.started = true;

        // Start both operations - the first to complete wins
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
    fn test_when_any_basic() {
        // Both complete immediately with same type, first one wins
        let result = sync_wait(when_any(just(1), just(2))).unwrap();
        // With inline execution, sender1 completes first
        assert_eq!(result, 1);
    }

    #[test]
    fn test_when_any_first_wins() {
        let result = sync_wait(when_any(just(42), just(0))).unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn test_when_any_strings() {
        let result = sync_wait(when_any(just("first"), just("second"))).unwrap();
        assert_eq!(result, "first");
    }

    #[test]
    fn test_when_any_unit() {
        let result = sync_wait(when_any(just(()), just(()))).unwrap();
        assert_eq!(result, ());
    }
}
