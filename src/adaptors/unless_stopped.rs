//! UnlessStopped adaptor - checks stop token before proceeding.
//!
//! `unless_stopped` checks whether stop has been requested before
//! continuing with the upstream sender's result.

use crate::stop::StopToken;
use crate::traits::{
    CompletionSignatures, Environment, OperationState, Receiver, ReceiverOf, ReceiverOfError,
    ReceiverOfStopped, Sender, SenderTo,
};
use std::marker::PhantomData;
use std::pin::Pin;

/// Check stop token and complete with stopped if stop was requested.
///
/// This adaptor checks the stop token when a value is received.
/// If stop has been requested, it completes with stopped instead
/// of forwarding the value.
///
/// # Example
///
/// ```rust
/// use exec_rs::factories::just;
/// use exec_rs::adaptors::unless_stopped;
/// use exec_rs::consumers::sync_wait;
///
/// // Normal case - stop not requested, value passes through
/// let result = sync_wait(unless_stopped(just(42))).unwrap();
/// assert_eq!(result, 42);
/// ```
pub fn unless_stopped<S>(sender: S) -> UnlessStopped<S> {
    UnlessStopped::new(sender)
}

/// A sender that checks the stop token.
#[derive(Debug, Clone)]
pub struct UnlessStopped<S> {
    sender: S,
}

impl<S> UnlessStopped<S> {
    /// Create a new UnlessStopped adaptor.
    pub fn new(sender: S) -> Self {
        Self { sender }
    }
}

/// Completion signatures for UnlessStopped.
pub struct UnlessStoppedCompletions<V, E>(PhantomData<(V, E)>);

impl<V, E> CompletionSignatures for UnlessStoppedCompletions<V, E> {
    type Value = V;
    type Error = E;
    const SENDS_STOPPED: bool = true;
}

impl<S, V, E> Sender for UnlessStopped<S>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
{
    type Completions = UnlessStoppedCompletions<V, E>;
}

impl<S, R, V, E> SenderTo<R> for UnlessStopped<S>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>
        + SenderTo<UnlessStoppedReceiver<R, V, E>>,
    R: ReceiverOf<V> + ReceiverOfError<E> + ReceiverOfStopped,
    R::Env: Environment,
    <R::Env as Environment>::StopToken: StopToken,
{
    type Operation = UnlessStoppedOperation<S, R, V, E>;

    fn connect(self, receiver: R) -> Self::Operation {
        let inner_receiver = UnlessStoppedReceiver {
            receiver: Some(receiver),
            _phantom: PhantomData,
        };
        UnlessStoppedOperation {
            inner: self.sender.connect(inner_receiver),
        }
    }
}

/// Receiver for UnlessStopped adaptor.
pub struct UnlessStoppedReceiver<R, V, E> {
    receiver: Option<R>,
    _phantom: PhantomData<(V, E)>,
}

impl<R: Receiver, V, E> Receiver for UnlessStoppedReceiver<R, V, E> {
    type Env = R::Env;

    fn get_env(&self) -> &Self::Env {
        self.receiver.as_ref().unwrap().get_env()
    }
}

impl<R, V, E> ReceiverOf<V> for UnlessStoppedReceiver<R, V, E>
where
    R: ReceiverOf<V> + ReceiverOfStopped,
    R::Env: Environment,
    <R::Env as Environment>::StopToken: StopToken,
{
    fn set_value(mut self, value: V) {
        if let Some(receiver) = self.receiver.take() {
            // Check the stop token
            let stop_token = receiver.get_env().get_stop_token();
            if stop_token.stop_requested() {
                receiver.set_stopped();
            } else {
                receiver.set_value(value);
            }
        }
    }
}

impl<R, V, E> ReceiverOfError<E> for UnlessStoppedReceiver<R, V, E>
where
    R: ReceiverOfError<E>,
{
    fn set_error(mut self, error: E) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_error(error);
        }
    }
}

impl<R, V, E> ReceiverOfStopped for UnlessStoppedReceiver<R, V, E>
where
    R: ReceiverOfStopped,
{
    fn set_stopped(mut self) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_stopped();
        }
    }
}

/// Operation state for UnlessStopped.
pub struct UnlessStoppedOperation<S, R, V, E>
where
    S: SenderTo<UnlessStoppedReceiver<R, V, E>>,
    R: Receiver,
{
    inner: S::Operation,
}

impl<S, R, V, E> OperationState for UnlessStoppedOperation<S, R, V, E>
where
    S: SenderTo<UnlessStoppedReceiver<R, V, E>>,
    R: Receiver,
{
    fn start(self: Pin<&mut Self>) {
        unsafe {
            let this = self.get_unchecked_mut();
            Pin::new_unchecked(&mut this.inner).start();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consumers::sync_wait;
    use crate::factories::just;
    use crate::stop::{InplaceStopSource, InplaceStopToken, StopSource};
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[test]
    fn test_unless_stopped_passthrough() {
        // When stop is not requested, value passes through
        let result = sync_wait(unless_stopped(just(42))).unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn test_unless_stopped_string() {
        let result = sync_wait(unless_stopped(just("hello"))).unwrap();
        assert_eq!(result, "hello");
    }

    /// Test that unless_stopped actually stops when stop is requested
    #[test]
    fn test_unless_stopped_actually_stops() {
        use crate::traits::{OperationState, Receiver, SenderTo};

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
        }

        impl Receiver for TestReceiver {
            type Env = TestEnv;

            fn get_env(&self) -> &Self::Env {
                &self.env
            }
        }

        impl ReceiverOf<i32> for TestReceiver {
            fn set_value(self, _value: i32) {
                self.value_called.store(true, Ordering::SeqCst);
            }
        }

        impl ReceiverOfError<std::convert::Infallible> for TestReceiver {
            fn set_error(self, _: std::convert::Infallible) {}
        }

        impl ReceiverOfStopped for TestReceiver {
            fn set_stopped(self) {
                self.stopped_called.store(true, Ordering::SeqCst);
            }
        }

        // Test: With stop requested, unless_stopped should complete with stopped
        let source = InplaceStopSource::new();
        let token = source.get_token();

        // Request stop BEFORE starting
        source.request_stop();

        let value_called = Arc::new(AtomicBool::new(false));
        let stopped_called = Arc::new(AtomicBool::new(false));

        let receiver = TestReceiver {
            env: TestEnv { stop_token: token },
            value_called: value_called.clone(),
            stopped_called: stopped_called.clone(),
        };

        // Create unless_stopped(just(42)) and connect to our test receiver
        let sender = unless_stopped(just(42));
        let mut op = sender.connect(receiver);
        let pinned = unsafe { Pin::new_unchecked(&mut op) };
        pinned.start();

        assert!(
            !value_called.load(Ordering::SeqCst),
            "set_value should NOT be called when stop was requested"
        );
        assert!(
            stopped_called.load(Ordering::SeqCst),
            "set_stopped should be called when stop was requested"
        );
    }

    /// Test that unless_stopped passes value when stop is NOT requested
    #[test]
    fn test_unless_stopped_passes_value_when_not_stopped() {
        use crate::traits::{OperationState, Receiver, SenderTo};

        struct TestEnv {
            stop_token: InplaceStopToken,
        }

        impl Environment for TestEnv {
            type StopToken = InplaceStopToken;

            fn get_stop_token(&self) -> Self::StopToken {
                self.stop_token.clone()
            }
        }

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

        impl ReceiverOfError<std::convert::Infallible> for TestReceiver {
            fn set_error(self, _: std::convert::Infallible) {}
        }

        impl ReceiverOfStopped for TestReceiver {
            fn set_stopped(self) {
                self.stopped_called.store(true, Ordering::SeqCst);
            }
        }

        // Test: Without stop requested, value should pass through
        let source = InplaceStopSource::new();
        let token = source.get_token();
        // Do NOT request stop

        let value_called = Arc::new(AtomicBool::new(false));
        let stopped_called = Arc::new(AtomicBool::new(false));
        let value_received = Arc::new(std::sync::Mutex::new(None));

        let receiver = TestReceiver {
            env: TestEnv { stop_token: token },
            value_called: value_called.clone(),
            stopped_called: stopped_called.clone(),
            value_received: value_received.clone(),
        };

        let sender = unless_stopped(just(42));
        let mut op = sender.connect(receiver);
        let pinned = unsafe { Pin::new_unchecked(&mut op) };
        pinned.start();

        assert!(
            value_called.load(Ordering::SeqCst),
            "set_value should be called when stop was NOT requested"
        );
        assert!(
            !stopped_called.load(Ordering::SeqCst),
            "set_stopped should NOT be called when stop was NOT requested"
        );
        assert_eq!(*value_received.lock().unwrap(), Some(42));
    }
}
