//! Scheduler trait for execution contexts.
//!
//! A scheduler is a handle to an execution context that can create senders
//! which complete on that context. This allows work to be scheduled on
//! specific threads, thread pools, or other execution resources.

use crate::traits::Sender;

/// The scheduler trait.
///
/// A scheduler represents an execution context and provides a way to
/// schedule work on it via the `schedule` method.
///
/// # Requirements
///
/// Schedulers must be:
/// - `Clone`: Schedulers can be copied to allow sharing
/// - `PartialEq + Eq`: Schedulers can be compared for equality
///
/// Two schedulers are equal if work scheduled on them will execute on
/// the same execution context.
pub trait Scheduler: Clone + PartialEq + Eq {
    /// The sender type returned by `schedule`.
    type Sender: Sender;

    /// Create a sender that completes on this scheduler's execution context.
    ///
    /// The returned sender, when started, will transition to the scheduler's
    /// execution context before completing with a value signal.
    fn schedule(&self) -> Self::Sender;
}

/// Extension trait for schedulers.
pub trait SchedulerExt: Scheduler {
    /// Schedule a sender to run on this scheduler.
    ///
    /// This is equivalent to `on(scheduler, sender)` but called as a method.
    fn run<S: Sender>(&self, sender: S) -> crate::adaptors::On<Self, S>
    where
        Self: Sized,
    {
        crate::adaptors::On::new(self.clone(), sender)
    }
}

impl<S: Scheduler> SchedulerExt for S {}

/// A timed scheduler that can schedule work at specific times.
///
/// This extends the basic scheduler with time-based scheduling capabilities.
pub trait TimedScheduler: Scheduler {
    /// The time point type used by this scheduler.
    type TimePoint: Ord + Clone;
    /// The duration type used by this scheduler.
    type Duration;
    /// The sender type for timed operations.
    type TimedSender: Sender;

    /// Get the current time according to this scheduler's clock.
    fn now(&self) -> Self::TimePoint;

    /// Schedule a sender to complete at a specific time point.
    fn schedule_at(&self, time: Self::TimePoint) -> Self::TimedSender;

    /// Schedule a sender to complete after a duration.
    fn schedule_after(&self, duration: Self::Duration) -> Self::TimedSender;
}

/// Marker trait for schedulers that execute work inline.
///
/// Inline schedulers execute work immediately on the calling thread
/// rather than deferring to another execution context.
pub trait InlineSchedulerMarker: Scheduler {}

/// Marker trait for schedulers that guarantee single-threaded execution.
pub trait SingleThreadedScheduler: Scheduler {}

/// Marker trait for schedulers that support work stealing.
pub trait WorkStealingScheduler: Scheduler {}
