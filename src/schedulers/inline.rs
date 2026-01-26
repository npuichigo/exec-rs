//! Inline scheduler - executes work immediately on the calling thread.

use crate::traits::{
    CompletionSignatures, InlineSchedulerMarker, OperationState, ReceiverOf, Scheduler, Sender,
    SenderTo,
};
use std::marker::PhantomData;
use std::pin::Pin;

/// A scheduler that executes work immediately on the calling thread.
///
/// This is the simplest scheduler - `schedule()` returns a sender that
/// completes immediately when started, calling `set_value()` with no arguments.
///
/// # Example
///
/// ```rust
/// use exec_rs::schedulers::InlineScheduler;
/// use exec_rs::traits::Scheduler;
/// use exec_rs::consumers::sync_wait;
///
/// let scheduler = InlineScheduler;
/// let sender = scheduler.schedule();
/// let result = sync_wait(sender).unwrap();
/// assert_eq!(result, ());
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct InlineScheduler;

impl Scheduler for InlineScheduler {
    type Sender = InlineScheduleSender;

    fn schedule(&self) -> Self::Sender {
        InlineScheduleSender
    }
}

impl InlineSchedulerMarker for InlineScheduler {}

/// Completion signatures for inline schedule sender.
pub struct InlineScheduleCompletions;

impl CompletionSignatures for InlineScheduleCompletions {
    type Value = ();
    type Error = std::convert::Infallible;
    const SENDS_STOPPED: bool = false;
}

/// The sender returned by `InlineScheduler::schedule()`.
#[derive(Debug, Clone, Copy, Default)]
pub struct InlineScheduleSender;

impl Sender for InlineScheduleSender {
    type Completions = InlineScheduleCompletions;
}

impl<R> SenderTo<R> for InlineScheduleSender
where
    R: ReceiverOf<()>,
{
    type Operation = InlineScheduleOperation<R>;

    fn connect(self, receiver: R) -> Self::Operation {
        InlineScheduleOperation {
            receiver: Some(receiver),
        }
    }
}

/// Operation state for inline schedule sender.
pub struct InlineScheduleOperation<R> {
    receiver: Option<R>,
}

impl<R: ReceiverOf<()>> OperationState for InlineScheduleOperation<R> {
    fn start(self: Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };
        if let Some(receiver) = this.receiver.take() {
            receiver.set_value(());
        }
    }
}

/// A typed inline scheduler that produces a specific value.
///
/// This is useful for testing or when you need a scheduler that
/// provides a value immediately.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypedInlineScheduler<T> {
    value: T,
}

impl<T: Clone> TypedInlineScheduler<T> {
    /// Create a new typed inline scheduler.
    pub fn new(value: T) -> Self {
        Self { value }
    }
}

/// Sender for typed inline scheduler.
pub struct TypedInlineScheduleSender<T> {
    value: T,
}

/// Completions for typed inline schedule sender.
pub struct TypedInlineScheduleCompletions<T>(PhantomData<T>);

impl<T> CompletionSignatures for TypedInlineScheduleCompletions<T> {
    type Value = T;
    type Error = std::convert::Infallible;
    const SENDS_STOPPED: bool = false;
}

impl<T: Clone> Sender for TypedInlineScheduleSender<T> {
    type Completions = TypedInlineScheduleCompletions<T>;
}

impl<T: Clone + PartialEq + Eq> Scheduler for TypedInlineScheduler<T> {
    type Sender = TypedInlineScheduleSender<T>;

    fn schedule(&self) -> Self::Sender {
        TypedInlineScheduleSender {
            value: self.value.clone(),
        }
    }
}

impl<T: Clone, R> SenderTo<R> for TypedInlineScheduleSender<T>
where
    R: ReceiverOf<T>,
{
    type Operation = TypedInlineScheduleOperation<T, R>;

    fn connect(self, receiver: R) -> Self::Operation {
        TypedInlineScheduleOperation {
            value: Some(self.value),
            receiver: Some(receiver),
        }
    }
}

/// Operation state for typed inline schedule sender.
pub struct TypedInlineScheduleOperation<T, R> {
    value: Option<T>,
    receiver: Option<R>,
}

impl<T, R: ReceiverOf<T>> OperationState for TypedInlineScheduleOperation<T, R> {
    fn start(self: Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };
        if let (Some(value), Some(receiver)) = (this.value.take(), this.receiver.take()) {
            receiver.set_value(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consumers::sync_wait;

    #[test]
    fn test_inline_scheduler() {
        let scheduler = InlineScheduler;
        let sender = scheduler.schedule();
        let result = sync_wait(sender).unwrap();
        assert_eq!(result, ());
    }

    #[test]
    fn test_inline_scheduler_equality() {
        let s1 = InlineScheduler;
        let s2 = InlineScheduler;
        assert_eq!(s1, s2);
    }
}
