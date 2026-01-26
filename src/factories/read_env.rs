//! read_env - A sender that reads a value from the receiver's environment.
//!
//! This factory creates a sender that queries the receiver's environment
//! and completes with the result.

use crate::traits::{
    CompletionSignatures, Environment, OperationState, Receiver, ReceiverOf, Sender, SenderTo,
};
use std::marker::PhantomData;
use std::pin::Pin;

/// A query trait for reading values from an environment.
///
/// Implement this trait for custom query types.
pub trait EnvQuery<E: Environment> {
    /// The result type of the query.
    type Output;

    /// Execute the query on the given environment.
    fn query(env: &E) -> Self::Output;
}

/// Query for getting the stop token from an environment.
#[derive(Debug, Clone, Copy, Default)]
pub struct GetStopToken;

impl<E: Environment> EnvQuery<E> for GetStopToken {
    type Output = E::StopToken;

    fn query(env: &E) -> Self::Output {
        env.get_stop_token()
    }
}

/// Query for getting the scheduler from an environment.
#[derive(Debug, Clone, Copy, Default)]
pub struct GetScheduler;

impl<E> EnvQuery<E> for GetScheduler
where
    E: Environment + crate::traits::GetScheduler,
{
    type Output = <E as crate::traits::GetScheduler>::Scheduler;

    fn query(env: &E) -> Self::Output {
        <E as crate::traits::GetScheduler>::get_scheduler(env)
    }
}

/// Query for getting the delegation scheduler from an environment.
#[derive(Debug, Clone, Copy, Default)]
pub struct GetDelegationScheduler;

impl<E> EnvQuery<E> for GetDelegationScheduler
where
    E: Environment + crate::traits::GetDelegationScheduler,
{
    type Output = <E as crate::traits::GetDelegationScheduler>::Scheduler;

    fn query(env: &E) -> Self::Output {
        <E as crate::traits::GetDelegationScheduler>::get_delegation_scheduler(env)
    }
}

/// Creates a sender that reads a value from the receiver's environment.
///
/// When started, this sender queries the receiver's environment using the
/// provided query and completes with the result.
///
/// # Example
///
/// ```rust,ignore
/// use exec_rs::factories::{read_env, GetStopToken};
/// use exec_rs::consumers::sync_wait;
///
/// // Read the stop token from the environment
/// let sender = read_env(GetStopToken);
/// ```
pub fn read_env<Q>(query: Q) -> ReadEnv<Q> {
    ReadEnv { query }
}

/// A sender that reads a value from the environment.
#[derive(Debug, Clone)]
pub struct ReadEnv<Q> {
    query: Q,
}

/// Completion signatures for ReadEnv.
pub struct ReadEnvCompletions<T>(PhantomData<T>);

impl<T> CompletionSignatures for ReadEnvCompletions<T> {
    type Value = T;
    type Error = std::convert::Infallible;
    const SENDS_STOPPED: bool = false;
}

impl<Q> Sender for ReadEnv<Q>
where
    Q: Clone,
{
    // Note: The actual completion type depends on the receiver's environment,
    // which we don't know at this point. We use a placeholder.
    type Completions = ReadEnvCompletions<()>;
}

impl<Q, R, T> SenderTo<R> for ReadEnv<Q>
where
    Q: EnvQuery<R::Env, Output = T> + Clone,
    R: Receiver + ReceiverOf<T>,
    R::Env: Environment,
{
    type Operation = ReadEnvOperation<Q, R>;

    fn connect(self, receiver: R) -> Self::Operation {
        ReadEnvOperation {
            query: self.query,
            receiver: Some(receiver),
        }
    }
}

/// Operation state for ReadEnv.
pub struct ReadEnvOperation<Q, R> {
    query: Q,
    receiver: Option<R>,
}

impl<Q, R, T> OperationState for ReadEnvOperation<Q, R>
where
    Q: EnvQuery<R::Env, Output = T>,
    R: Receiver + ReceiverOf<T>,
    R::Env: Environment,
{
    fn start(self: Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };
        if let Some(receiver) = this.receiver.take() {
            let value = Q::query(receiver.get_env());
            receiver.set_value(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stop::{InplaceStopSource, InplaceStopToken, NeverStopToken, StopSource, StopToken};
    use crate::traits::{ReceiverOfError, ReceiverOfStopped};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[test]
    fn test_read_env_stop_token() {
        // Test environment with a stop token
        struct TestEnv {
            stop_token: InplaceStopToken,
        }

        impl Environment for TestEnv {
            type StopToken = InplaceStopToken;

            fn get_stop_token(&self) -> Self::StopToken {
                self.stop_token.clone()
            }
        }

        // Test receiver
        struct TestReceiver {
            env: TestEnv,
            received_token: Arc<std::sync::Mutex<Option<InplaceStopToken>>>,
        }

        impl Receiver for TestReceiver {
            type Env = TestEnv;

            fn get_env(&self) -> &Self::Env {
                &self.env
            }
        }

        impl ReceiverOf<InplaceStopToken> for TestReceiver {
            fn set_value(self, value: InplaceStopToken) {
                *self.received_token.lock().unwrap() = Some(value);
            }
        }

        impl ReceiverOfError<std::convert::Infallible> for TestReceiver {
            fn set_error(self, _: std::convert::Infallible) {}
        }

        impl ReceiverOfStopped for TestReceiver {
            fn set_stopped(self) {}
        }

        // Create a stop source and token
        let source = InplaceStopSource::new();
        let token = source.get_token();
        source.request_stop();

        let received = Arc::new(std::sync::Mutex::new(None));

        let receiver = TestReceiver {
            env: TestEnv {
                stop_token: token.clone(),
            },
            received_token: received.clone(),
        };

        // Create and run the sender
        let sender = read_env(GetStopToken);
        let mut op = <ReadEnv<GetStopToken> as SenderTo<TestReceiver>>::connect(sender, receiver);
        let pinned = unsafe { Pin::new_unchecked(&mut op) };
        pinned.start();

        // Verify we received the stop token
        let received_token = received.lock().unwrap().take().unwrap();
        assert!(received_token.stop_requested());
    }

    #[test]
    fn test_read_env_never_stop_token() {
        // Use the empty environment which returns NeverStopToken
        let received = Arc::new(AtomicBool::new(false));
        let received_clone = received.clone();

        struct TestReceiver {
            received: Arc<AtomicBool>,
        }

        impl Receiver for TestReceiver {
            type Env = ();

            fn get_env(&self) -> &Self::Env {
                &()
            }
        }

        impl ReceiverOf<NeverStopToken> for TestReceiver {
            fn set_value(self, token: NeverStopToken) {
                assert!(!token.stop_requested());
                assert!(!token.stop_possible());
                self.received.store(true, Ordering::SeqCst);
            }
        }

        impl ReceiverOfError<std::convert::Infallible> for TestReceiver {
            fn set_error(self, _: std::convert::Infallible) {}
        }

        impl ReceiverOfStopped for TestReceiver {
            fn set_stopped(self) {}
        }

        let receiver = TestReceiver {
            received: received_clone,
        };

        let sender = read_env(GetStopToken);
        let mut op = <ReadEnv<GetStopToken> as SenderTo<TestReceiver>>::connect(sender, receiver);
        let pinned = unsafe { Pin::new_unchecked(&mut op) };
        pinned.start();

        assert!(received.load(Ordering::SeqCst));
    }
}
