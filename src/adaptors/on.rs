//! On adaptor - run a sender on a specific scheduler.

use crate::traits::{
    CompletionSignatures, OperationState, Receiver, ReceiverOf, ReceiverOfError, ReceiverOfStopped,
    Scheduler, Sender, SenderTo,
};
use std::marker::PhantomData;
use std::pin::{pin, Pin};

/// Run a sender on a specific scheduler.
///
/// This transitions to the scheduler's execution context before starting
/// the sender, and the sender's completion happens on that context.
pub fn on<Sch, S>(scheduler: Sch, sender: S) -> On<Sch, S> {
    On::new(scheduler, sender)
}

/// A sender that runs on a specific scheduler.
#[derive(Debug, Clone)]
pub struct On<Sch, S> {
    scheduler: Sch,
    sender: S,
}

impl<Sch, S> On<Sch, S> {
    /// Create a new On adaptor.
    pub fn new(scheduler: Sch, sender: S) -> Self {
        Self { scheduler, sender }
    }
}

impl<Sch, S> Sender for On<Sch, S>
where
    Sch: Scheduler,
    S: Sender,
{
    type Completions = S::Completions;
}

impl<Sch, S, R> SenderTo<R> for On<Sch, S>
where
    Sch: Scheduler,
    Sch::Sender: SenderTo<OnScheduleReceiver<S, R>>,
    S: Sender + SenderTo<R>,
    R: ReceiverOf<<S::Completions as CompletionSignatures>::Value>
        + ReceiverOfError<<S::Completions as CompletionSignatures>::Error>
        + ReceiverOfStopped,
{
    type Operation = OnOperation<Sch, S, R>;

    fn connect(self, receiver: R) -> Self::Operation {
        let schedule_sender = self.scheduler.schedule();
        let schedule_receiver = OnScheduleReceiver {
            sender: Some(self.sender),
            receiver: Some(receiver),
        };
        OnOperation {
            inner: schedule_sender.connect(schedule_receiver),
            _phantom: PhantomData,
        }
    }
}

/// Receiver for the schedule sender in On.
pub struct OnScheduleReceiver<S, R> {
    sender: Option<S>,
    receiver: Option<R>,
}

impl<S, R: Receiver> Receiver for OnScheduleReceiver<S, R> {
    type Env = R::Env;

    fn get_env(&self) -> &Self::Env {
        self.receiver.as_ref().unwrap().get_env()
    }
}

impl<S, R> ReceiverOf<()> for OnScheduleReceiver<S, R>
where
    S: Sender + SenderTo<R>,
    R: ReceiverOf<<S::Completions as CompletionSignatures>::Value>
        + ReceiverOfError<<S::Completions as CompletionSignatures>::Error>
        + ReceiverOfStopped,
{
    fn set_value(mut self, _: ()) {
        if let (Some(sender), Some(receiver)) = (self.sender.take(), self.receiver.take()) {
            let mut op = pin!(sender.connect(receiver));
            op.as_mut().start();
        }
    }
}

impl<S, R, E> ReceiverOfError<E> for OnScheduleReceiver<S, R>
where
    R: ReceiverOfError<E>,
{
    fn set_error(mut self, error: E) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_error(error);
        }
    }
}

impl<S, R> ReceiverOfStopped for OnScheduleReceiver<S, R>
where
    R: ReceiverOfStopped,
{
    fn set_stopped(mut self) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_stopped();
        }
    }
}

/// Operation state for On.
pub struct OnOperation<Sch, S, R>
where
    Sch: Scheduler,
    Sch::Sender: SenderTo<OnScheduleReceiver<S, R>>,
    R: Receiver,
{
    inner: <Sch::Sender as SenderTo<OnScheduleReceiver<S, R>>>::Operation,
    _phantom: PhantomData<(S, R)>,
}

impl<Sch, S, R> OperationState for OnOperation<Sch, S, R>
where
    Sch: Scheduler,
    Sch::Sender: SenderTo<OnScheduleReceiver<S, R>>,
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
    use crate::schedulers::InlineScheduler;

    #[test]
    fn test_on_inline() {
        let result = sync_wait(on(InlineScheduler, just(42))).unwrap();
        assert_eq!(result, 42);
    }
}
