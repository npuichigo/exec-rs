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
    use crate::adaptors::then;
    use crate::consumers::sync_wait;
    use crate::factories::schedule;
    use crate::traits::Sender;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    // ========================================================================
    // test_inline_scheduler.cpp equivalents
    // ========================================================================

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

    #[test]
    fn test_inline_scheduler_is_scheduler() {
        fn assert_scheduler<S: Scheduler>(_: &S) {}
        let scheduler = InlineScheduler;
        assert_scheduler(&scheduler);
    }

    #[test]
    fn test_inline_scheduler_with_then() {
        let scheduler = InlineScheduler;
        let result = sync_wait(then(schedule(scheduler), |_| 42)).unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn test_inline_scheduler_clone() {
        let scheduler1 = InlineScheduler;
        let scheduler2 = scheduler1;
        assert_eq!(scheduler1, scheduler2);
    }

    #[test]
    fn test_inline_scheduler_runs_immediately() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let scheduler = InlineScheduler;
        let _result = sync_wait(then(schedule(scheduler), move |_| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
            42
        }))
        .unwrap();

        // Should have run immediately
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_inline_schedule_sender_is_sender() {
        fn assert_sender<S: Sender>(_: &S) {}
        let scheduler = InlineScheduler;
        let sender = scheduler.schedule();
        assert_sender(&sender);
    }

    #[test]
    fn test_typed_inline_scheduler() {
        let scheduler = TypedInlineScheduler::new(42);
        let sender = scheduler.schedule();
        let result = sync_wait(sender).unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn test_typed_inline_scheduler_equality() {
        let s1 = TypedInlineScheduler::new(42);
        let s2 = TypedInlineScheduler::new(42);
        let s3 = TypedInlineScheduler::new(0);
        assert_eq!(s1, s2);
        assert_ne!(s1, s3);
    }
}
