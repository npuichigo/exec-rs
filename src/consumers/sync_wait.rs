//! Synchronous wait consumer.
//!
//! `sync_wait` blocks the current thread until a sender completes, returning
//! the result.
//!
//! This implementation is allocation-free, matching C++ stdexec's design.
//! The state lives on the stack and the receiver holds a raw pointer to it.
//! This is safe because sync_wait blocks until completion.

use crate::schedulers::RunLoop;
use crate::stop::InplaceStopToken;
use crate::traits::{
    CompletionSignatures, Environment, OperationState, Receiver, ReceiverOf, ReceiverOfError,
    ReceiverOfStopped, Sender, SenderTo,
};
use std::cell::UnsafeCell;
use std::pin::pin;

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
        // State lives on the stack - no heap allocation!
        let state = SyncWaitState::<V, E>::new();

        // Create the receiver with a raw pointer to the stack-allocated state.
        // SAFETY: This is safe because:
        // 1. state lives on this function's stack frame
        // 2. run_loop.run() blocks until a completion signal is received
        // 3. The receiver is consumed when set_value/set_error/set_stopped is called
        // 4. After run_loop.run() returns, no references to state exist
        let receiver = SyncWaitReceiver {
            state: &state as *const SyncWaitState<V, E>,
        };

        // Connect and start
        let mut op = pin!(self.connect(receiver));
        op.as_mut().start();

        // Run the loop until completion - this blocks!
        state.run_loop.run();

        // Extract the result
        // SAFETY: The run_loop only finishes when a completion signal is received,
        // at which point result is set. No other references exist.
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

/// State for sync_wait - lives on the stack.
struct SyncWaitState<V, E> {
    run_loop: RunLoop,
    result: UnsafeCell<Option<Result<V, SyncWaitError<E>>>>,
}

// SAFETY: The result is only accessed after the run_loop completes,
// and only from one thread. The raw pointer in SyncWaitReceiver is
// valid for the duration of sync_wait.
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
///
/// Holds a raw pointer to stack-allocated state. This is safe because
/// sync_wait blocks until the operation completes, and the receiver is
/// consumed when completion signals are delivered.
#[derive(Clone)]
pub struct SyncWaitReceiver<V, E> {
    state: *const SyncWaitState<V, E>,
}

// SAFETY: The pointer points to a SyncWaitState that is Send+Sync,
// and the receiver is only used while sync_wait is blocking.
unsafe impl<V: Send, E: Send> Send for SyncWaitReceiver<V, E> {}
unsafe impl<V: Send, E: Send> Sync for SyncWaitReceiver<V, E> {}

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
        // SAFETY: The pointer is valid because sync_wait is still blocking
        let state = unsafe { &*self.state };
        unsafe {
            *state.result.get() = Some(Ok(value));
        }
        state.run_loop.finish();
    }
}

impl<V, E> ReceiverOfError<E> for SyncWaitReceiver<V, E>
where
    V: 'static,
    E: 'static,
{
    fn set_error(self, error: E) {
        let state = unsafe { &*self.state };
        unsafe {
            *state.result.get() = Some(Err(SyncWaitError::Error(error)));
        }
        state.run_loop.finish();
    }
}

impl<V, E> ReceiverOfStopped for SyncWaitReceiver<V, E>
where
    V: 'static,
    E: 'static,
{
    fn set_stopped(self) {
        let state = unsafe { &*self.state };
        unsafe {
            *state.result.get() = Some(Err(SyncWaitError::Stopped));
        }
        state.run_loop.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adaptors::{then, when_all};
    use crate::factories::{just, just_error, just_stopped, schedule};
    use crate::schedulers::InlineScheduler;
    use crate::traits::SenderExt;

    // ========================================================================
    // test_sync_wait.cpp equivalents
    // ========================================================================

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

    #[test]
    fn test_sync_wait_tuple() {
        let result = sync_wait(just((1, 2, 3))).unwrap();
        assert_eq!(result, (1, 2, 3));
    }

    #[test]
    fn test_sync_wait_unit() {
        let result = sync_wait(just(())).unwrap();
        assert_eq!(result, ());
    }

    #[test]
    fn test_sync_wait_move_only_value() {
        let boxed = Box::new(42);
        let result = sync_wait(just(boxed)).unwrap();
        assert_eq!(*result, 42);
    }

    #[test]
    fn test_sync_wait_with_then() {
        let result = sync_wait(then(just(21), |x| x * 2)).unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn test_sync_wait_with_when_all() {
        let result = sync_wait(when_all(just(1), just(2))).unwrap();
        assert_eq!(result, (1, 2));
    }

    #[test]
    fn test_sync_wait_with_schedule() {
        let scheduler = InlineScheduler;
        let result = sync_wait(then(schedule(scheduler), |_| 42)).unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn test_sync_wait_complex_type() {
        let vec = vec![1, 2, 3, 4, 5];
        let result = sync_wait(just(vec)).unwrap();
        assert_eq!(result, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_sync_wait_option() {
        let result = sync_wait(just(Some(42))).unwrap();
        assert_eq!(result, Some(42));
    }

    #[test]
    fn test_sync_wait_nested_result() {
        let result = sync_wait(just(Ok::<i32, &str>(42))).unwrap();
        assert_eq!(result, Ok(42));
    }

    #[test]
    fn test_sync_wait_error_preserves_type() {
        #[derive(Debug, PartialEq)]
        struct CustomError(i32);

        let result = sync_wait(just_error::<_>(CustomError(42)));
        match result {
            Err(SyncWaitError::Error(e)) => assert_eq!(e, CustomError(42)),
            _ => panic!("Expected error"),
        }
    }

    #[test]
    fn test_sync_wait_chained_operations() {
        let result = sync_wait(
            just(1)
                .then(|x| x + 1)
                .then(|x| x * 2)
                .then(|x| x + 3)
                .then(|x| format!("result: {}", x)),
        )
        .unwrap();
        assert_eq!(result, "result: 7");
    }
}
