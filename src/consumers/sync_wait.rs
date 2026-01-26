//! Synchronous wait consumer.
//!
//! `sync_wait` blocks the current thread until a sender completes, returning
//! the result.

use crate::schedulers::RunLoop;
use crate::stop::InplaceStopToken;
use crate::traits::{
    CompletionSignatures, Environment, OperationState, Receiver, ReceiverOf, ReceiverOfError,
    ReceiverOfStopped, Sender, SenderTo,
};
use std::cell::UnsafeCell;
use std::pin::pin;
use std::sync::Arc;

/// Error type for sync_wait.
#[derive(Debug)]
pub enum SyncWaitError<E> {
    /// The sender completed with an error.
    Error(E),
    /// The sender was stopped/cancelled.
    Stopped,
}

impl<E: std::fmt::Display> std::fmt::Display for SyncWaitError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncWaitError::Error(e) => write!(f, "sync_wait error: {}", e),
            SyncWaitError::Stopped => write!(f, "sync_wait: operation was stopped"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for SyncWaitError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SyncWaitError::Error(e) => Some(e),
            SyncWaitError::Stopped => None,
        }
    }
}

/// A trait for senders that can be used with sync_wait.
///
/// This helps break the cyclic bounds by making the Value and Error types explicit.
pub trait SyncWaitable: Sender {
    /// The value type this sender produces.
    type Value: 'static;
    /// The error type this sender can produce.
    type Error: 'static;

    /// Execute sync_wait on this sender.
    fn sync_wait_impl(self) -> Result<Self::Value, SyncWaitError<Self::Error>>;
}

impl<S, V, E> SyncWaitable for S
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>
        + SenderTo<SyncWaitReceiver<V, E>>,
    V: 'static,
    E: 'static,
{
    type Value = V;
    type Error = E;

    fn sync_wait_impl(self) -> Result<V, SyncWaitError<E>> {
        // Create shared state
        let state = Arc::new(SyncWaitState::<V, E>::new());

        // Create the receiver
        let receiver = SyncWaitReceiver {
            state: state.clone(),
        };

        // Connect and start
        let mut op = pin!(self.connect(receiver));
        op.as_mut().start();

        // Run the loop until completion
        state.run_loop.run();

        // Extract the result
        // SAFETY: The run_loop only finishes when a completion signal is received,
        // at which point result is set
        unsafe {
            (*state.result.get())
                .take()
                .expect("sync_wait: sender did not complete")
        }
    }
}

/// Block until a sender completes, returning the result.
///
/// This function connects the sender to a receiver that captures the result,
/// starts the operation, and runs a `RunLoop` until completion.
///
/// # Returns
///
/// - `Ok(value)` if the sender completed with a value
/// - `Err(SyncWaitError::Error(e))` if the sender completed with an error
/// - `Err(SyncWaitError::Stopped)` if the sender was stopped
///
/// # Panics
///
/// Panics if the sender doesn't complete (this shouldn't happen with
/// well-behaved senders).
///
/// # Example
///
/// ```rust
/// use exec_rs::factories::just;
/// use exec_rs::consumers::sync_wait;
///
/// let result = sync_wait(just(42)).unwrap();
/// assert_eq!(result, 42);
/// ```
pub fn sync_wait<S: SyncWaitable>(sender: S) -> Result<S::Value, SyncWaitError<S::Error>> {
    sender.sync_wait_impl()
}

/// Shared state for sync_wait.
struct SyncWaitState<V, E> {
    run_loop: RunLoop,
    result: UnsafeCell<Option<Result<V, SyncWaitError<E>>>>,
}

// SAFETY: The result is only accessed after the run_loop completes,
// and only from one thread
unsafe impl<V: Send, E: Send> Send for SyncWaitState<V, E> {}
unsafe impl<V: Send, E: Send> Sync for SyncWaitState<V, E> {}

impl<V, E> SyncWaitState<V, E> {
    fn new() -> Self {
        Self {
            run_loop: RunLoop::new(),
            result: UnsafeCell::new(None),
        }
    }
}

/// The receiver used by sync_wait.
#[derive(Clone)]
pub struct SyncWaitReceiver<V, E> {
    state: Arc<SyncWaitState<V, E>>,
}

/// Environment for sync_wait receiver.
#[derive(Clone)]
pub struct SyncWaitEnv {
    stop_token: InplaceStopToken,
}

impl SyncWaitEnv {
    fn new() -> Self {
        Self {
            stop_token: InplaceStopToken::new_detached(),
        }
    }
}

impl Environment for SyncWaitEnv {
    type StopToken = InplaceStopToken;

    fn get_stop_token(&self) -> Self::StopToken {
        self.stop_token.clone()
    }
}

impl<V, E> Receiver for SyncWaitReceiver<V, E> {
    type Env = SyncWaitEnv;

    fn get_env(&self) -> &Self::Env {
        // Use a thread-local instead of a static
        thread_local! {
            static ENV: SyncWaitEnv = SyncWaitEnv::new();
        }
        // SAFETY: We return a reference that's valid for the lifetime of the receiver
        // This works because ENV is thread-local and we're single-threaded in sync_wait
        ENV.with(|env| unsafe { &*(env as *const SyncWaitEnv) })
    }
}

impl<V, E> ReceiverOf<V> for SyncWaitReceiver<V, E>
where
    V: 'static,
    E: 'static,
{
    fn set_value(self, value: V) {
        // SAFETY: We have exclusive access through self
        unsafe {
            *self.state.result.get() = Some(Ok(value));
        }
        self.state.run_loop.finish();
    }
}

impl<V, E> ReceiverOfError<E> for SyncWaitReceiver<V, E>
where
    V: 'static,
    E: 'static,
{
    fn set_error(self, error: E) {
        unsafe {
            *self.state.result.get() = Some(Err(SyncWaitError::Error(error)));
        }
        self.state.run_loop.finish();
    }
}

impl<V, E> ReceiverOfStopped for SyncWaitReceiver<V, E>
where
    V: 'static,
    E: 'static,
{
    fn set_stopped(self) {
        unsafe {
            *self.state.result.get() = Some(Err(SyncWaitError::Stopped));
        }
        self.state.run_loop.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::factories::{just, just_error, just_stopped};

    #[test]
    fn test_sync_wait_just() {
        let result = sync_wait(just(42)).unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn test_sync_wait_just_string() {
        let result = sync_wait(just("hello".to_string())).unwrap();
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_sync_wait_just_error() {
        let result: Result<(), SyncWaitError<&str>> = sync_wait(just_error("oops"));
        match result {
            Err(SyncWaitError::Error(e)) => assert_eq!(e, "oops"),
            _ => panic!("expected error"),
        }
    }

    #[test]
    fn test_sync_wait_just_stopped() {
        let result: Result<(), SyncWaitError<std::convert::Infallible>> = sync_wait(just_stopped());
        assert!(matches!(result, Err(SyncWaitError::Stopped)));
    }
}
