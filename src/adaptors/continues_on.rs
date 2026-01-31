//! ContinuesOn adaptor - continue execution on a specific scheduler.

use crate::traits::{
    CompletionSignatures, OperationState, Receiver, ReceiverOf, ReceiverOfError, ReceiverOfStopped,
    Scheduler, Sender, SenderTo,
};
use std::marker::PhantomData;
use std::pin::{pin, Pin};

/// Continue execution on a specific scheduler.
///
/// After the upstream sender completes with a value, the completion is
/// delivered on the specified scheduler's execution context.
pub fn continues_on<S, Sch>(sender: S, scheduler: Sch) -> ContinuesOn<S, Sch> {
    ContinuesOn::new(sender, scheduler)
}

/// A sender that continues on a specific scheduler.
#[derive(Debug, Clone)]
pub struct ContinuesOn<S, Sch> {
    sender: S,
    scheduler: Sch,
}

impl<S, Sch> ContinuesOn<S, Sch> {
    /// Create a new ContinuesOn adaptor.
    pub fn new(sender: S, scheduler: Sch) -> Self {
        Self { sender, scheduler }
    }
}

impl<S: Sender, Sch: Scheduler> Sender for ContinuesOn<S, Sch> {
    type Completions = S::Completions;
}

impl<S, Sch, R, V, E> SenderTo<R> for ContinuesOn<S, Sch>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>
        + SenderTo<ContinuesOnReceiver<Sch, R, V, E>>,
    Sch: Scheduler + Clone,
    Sch::Sender: SenderTo<ContinuesOnScheduleReceiver<R, V>>,
    R: ReceiverOf<V> + ReceiverOfError<E> + ReceiverOfStopped,
{
    type Operation = ContinuesOnOperation<S, Sch, R, V, E>;

    fn connect(self, receiver: R) -> Self::Operation {
        let inner_receiver = ContinuesOnReceiver {
            scheduler: self.scheduler,
            receiver: Some(receiver),
            _phantom: PhantomData,
        };
        ContinuesOnOperation {
            inner: self.sender.connect(inner_receiver),
        }
    }
}

/// Receiver for ContinuesOn adaptor.
pub struct ContinuesOnReceiver<Sch, R, V, E> {
    scheduler: Sch,
    receiver: Option<R>,
    _phantom: PhantomData<(V, E)>,
}

// SAFETY: ContinuesOnReceiver is Send if Sch and R are Send
unsafe impl<Sch: Send, R: Send, V, E> Send for ContinuesOnReceiver<Sch, R, V, E> {}
// SAFETY: ContinuesOnReceiver is Sync if Sch and R are Sync
unsafe impl<Sch: Sync, R: Sync, V, E> Sync for ContinuesOnReceiver<Sch, R, V, E> {}

impl<Sch, R: Receiver, V, E> Receiver for ContinuesOnReceiver<Sch, R, V, E> {
    type Env = R::Env;

    fn get_env(&self) -> &Self::Env {
        self.receiver.as_ref().unwrap().get_env()
    }
}

impl<Sch, R, V, E> ReceiverOf<V> for ContinuesOnReceiver<Sch, R, V, E>
where
    Sch: Scheduler,
    Sch::Sender: SenderTo<ContinuesOnScheduleReceiver<R, V>>,
    R: ReceiverOf<V>,
{
    fn set_value(mut self, value: V) {
        if let Some(receiver) = self.receiver.take() {
            let schedule_sender = self.scheduler.schedule();
            let schedule_receiver = ContinuesOnScheduleReceiver {
                value: Some(value),
                receiver: Some(receiver),
            };
            let mut op = pin!(schedule_sender.connect(schedule_receiver));
            op.as_mut().start();
        }
    }
}

impl<Sch, R, V, E> ReceiverOfError<E> for ContinuesOnReceiver<Sch, R, V, E>
where
    R: ReceiverOfError<E>,
{
    fn set_error(mut self, error: E) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_error(error);
        }
    }
}

impl<Sch, R, V, E> ReceiverOfStopped for ContinuesOnReceiver<Sch, R, V, E>
where
    R: ReceiverOfStopped,
{
    fn set_stopped(mut self) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_stopped();
        }
    }
}

/// Receiver for the schedule sender in ContinuesOn.
pub struct ContinuesOnScheduleReceiver<R, V> {
    value: Option<V>,
    receiver: Option<R>,
}

// SAFETY: ContinuesOnScheduleReceiver is Send if both R and V are Send
unsafe impl<R: Send, V: Send> Send for ContinuesOnScheduleReceiver<R, V> {}
// SAFETY: ContinuesOnScheduleReceiver is Sync if both R and V are Sync
unsafe impl<R: Sync, V: Sync> Sync for ContinuesOnScheduleReceiver<R, V> {}

impl<R: Receiver, V> Receiver for ContinuesOnScheduleReceiver<R, V> {
    type Env = R::Env;

    fn get_env(&self) -> &Self::Env {
        self.receiver.as_ref().unwrap().get_env()
    }
}

impl<R, V> ReceiverOf<()> for ContinuesOnScheduleReceiver<R, V>
where
    R: ReceiverOf<V>,
{
    fn set_value(mut self, _: ()) {
        if let (Some(value), Some(receiver)) = (self.value.take(), self.receiver.take()) {
            receiver.set_value(value);
        }
    }
}

impl<R, V, E> ReceiverOfError<E> for ContinuesOnScheduleReceiver<R, V>
where
    R: ReceiverOfError<E>,
{
    fn set_error(mut self, error: E) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_error(error);
        }
    }
}

impl<R, V> ReceiverOfStopped for ContinuesOnScheduleReceiver<R, V>
where
    R: ReceiverOfStopped,
{
    fn set_stopped(mut self) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_stopped();
        }
    }
}

/// Operation state for ContinuesOn.
pub struct ContinuesOnOperation<S, Sch, R, V, E>
where
    S: SenderTo<ContinuesOnReceiver<Sch, R, V, E>>,
    R: Receiver,
{
    inner: S::Operation,
}

impl<S, Sch, R, V, E> OperationState for ContinuesOnOperation<S, Sch, R, V, E>
where
    S: SenderTo<ContinuesOnReceiver<Sch, R, V, E>>,
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
    fn test_continues_on() {
        let result = sync_wait(continues_on(just(42), InlineScheduler)).unwrap();
        assert_eq!(result, 42);
    }
}
