//! Reschedule adaptor - reschedule execution on the environment's scheduler.
//!
//! `reschedule()` switches execution to the scheduler provided by the
//! receiver's environment. This is useful when you want to continue
//! execution on a specific scheduler that was set up in the pipeline.

use crate::traits::{
    CompletionSignatures, Environment, GetScheduler, OperationState, Receiver, ReceiverOf,
    ReceiverOfStopped, Scheduler, Sender, SenderTo,
};
use std::marker::PhantomData;
use std::pin::Pin;

/// Create a sender that reschedules on the environment's scheduler.
///
/// When this sender is started, it queries the receiver's environment for
/// a scheduler and schedules work on it. The sender completes with `()`
/// when the rescheduled work executes.
///
/// # Example
///
/// ```rust,ignore
/// use exec_rs::adaptors::{reschedule, then};
/// use exec_rs::factories::just;
/// use exec_rs::traits::{GetScheduler, WithScheduler};
/// use exec_rs::schedulers::InlineScheduler;
///
/// // The reschedule sender will use the scheduler from the environment
/// let sender = just(()).then(|_| {
///     // ... do some work ...
/// }).and_then(reschedule());
/// ```
pub fn reschedule() -> Reschedule {
    Reschedule { _private: () }
}

/// A sender that reschedules on the environment's scheduler.
#[derive(Clone, Debug)]
pub struct Reschedule {
    _private: (),
}

/// Completion signatures for reschedule.
pub struct RescheduleCompletions;

impl CompletionSignatures for RescheduleCompletions {
    type Value = ();
    type Error = std::convert::Infallible;
    const SENDS_STOPPED: bool = true;
}

impl Sender for Reschedule {
    type Completions = RescheduleCompletions;
}

impl<R> SenderTo<R> for Reschedule
where
    R: Receiver + ReceiverOf<()> + ReceiverOfStopped,
    R::Env: GetScheduler,
    <R::Env as GetScheduler>::Scheduler: Scheduler,
    <<R::Env as GetScheduler>::Scheduler as Scheduler>::Sender:
        SenderTo<RescheduleInnerReceiver<R>>,
{
    type Operation = RescheduleOperation<R>;

    fn connect(self, receiver: R) -> Self::Operation {
        RescheduleOperation {
            receiver: Some(receiver),
            inner_op: None,
            _phantom: PhantomData,
        }
    }
}

/// Operation state for reschedule.
pub struct RescheduleOperation<R>
where
    R: Receiver,
    R::Env: GetScheduler,
    <R::Env as GetScheduler>::Scheduler: Scheduler,
    <<R::Env as GetScheduler>::Scheduler as Scheduler>::Sender:
        SenderTo<RescheduleInnerReceiver<R>>,
{
    receiver: Option<R>,
    inner_op: Option<
        <<<R::Env as GetScheduler>::Scheduler as Scheduler>::Sender as SenderTo<
            RescheduleInnerReceiver<R>,
        >>::Operation,
    >,
    _phantom: PhantomData<R>,
}

impl<R> OperationState for RescheduleOperation<R>
where
    R: Receiver + ReceiverOf<()> + ReceiverOfStopped,
    R::Env: GetScheduler,
    <R::Env as GetScheduler>::Scheduler: Scheduler,
    <<R::Env as GetScheduler>::Scheduler as Scheduler>::Sender:
        SenderTo<RescheduleInnerReceiver<R>>,
{
    fn start(self: Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };

        if let Some(receiver) = this.receiver.take() {
            // Get the scheduler from the environment
            let env = receiver.get_env();
            let scheduler = env.get_scheduler();

            // Create the schedule sender and connect it
            let schedule_sender = scheduler.schedule();
            let inner_receiver = RescheduleInnerReceiver {
                downstream: receiver,
            };

            let op = schedule_sender.connect(inner_receiver);
            this.inner_op = Some(op);

            // Start the inner operation
            if let Some(ref mut op) = this.inner_op {
                unsafe {
                    Pin::new_unchecked(op).start();
                }
            }
        }
    }
}

/// Inner receiver that forwards completions from the scheduler.
pub struct RescheduleInnerReceiver<R> {
    downstream: R,
}

/// Environment for reschedule inner receiver - forwards from downstream.
pub struct RescheduleInnerEnv<'a, E> {
    inner: &'a E,
}

impl<'a, E: Environment> Environment for RescheduleInnerEnv<'a, E> {
    type StopToken = E::StopToken;

    fn get_stop_token(&self) -> Self::StopToken {
        self.inner.get_stop_token()
    }
}

impl<R: Receiver> Receiver for RescheduleInnerReceiver<R> {
    type Env = R::Env;

    fn get_env(&self) -> &Self::Env {
        self.downstream.get_env()
    }
}

impl<R: ReceiverOf<()>> ReceiverOf<()> for RescheduleInnerReceiver<R> {
    fn set_value(self, _: ()) {
        self.downstream.set_value(());
    }
}

impl<R: ReceiverOfStopped> ReceiverOfStopped for RescheduleInnerReceiver<R> {
    fn set_stopped(self) {
        self.downstream.set_stopped();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reschedule_basic() {
        // Test that reschedule creates a valid sender
        let sender = reschedule();
        // The sender should be cloneable
        let _cloned = sender.clone();
    }

    // Note: Full integration tests require a receiver with GetScheduler environment,
    // which is typically set up by adaptors like `on` or `starts_on`.
    // The reschedule adaptor is designed to work within pipelines where the
    // environment provides a scheduler, such as after `starts_on` or within
    // a scoped execution context.
}
