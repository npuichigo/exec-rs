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
{
    let receiver = DetachedReceiver;
    let mut op = sender.connect(receiver);

    // SAFETY: We're starting the operation and it will run to completion.
    // For inline execution, this happens synchronously.
    // For async execution, the operation state would need to be boxed
    // and kept alive until completion.
    unsafe {
        Pin::new_unchecked(&mut op).start();
    }

    // For inline execution, the operation has completed by now.
    // For async execution, we would need to leak/box the operation.
    // This simple implementation only works correctly with inline execution.
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

/// Receiver that discards all completions.
#[derive(Debug, Clone, Default)]
pub struct DetachedReceiver;

impl Receiver for DetachedReceiver {
    type Env = DetachedEnv;

    fn get_env(&self) -> &Self::Env {
        &DetachedEnv
    }
}

impl<V> ReceiverOf<V> for DetachedReceiver {
    fn set_value(self, _value: V) {
        // Discard the value
    }
}

impl<E> ReceiverOfError<E> for DetachedReceiver {
    fn set_error(self, _error: E) {
        // Discard the error
    }
}

impl ReceiverOfStopped for DetachedReceiver {
    fn set_stopped(self) {
        // Discard the stopped signal
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
