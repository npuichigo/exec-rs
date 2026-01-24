//! Timed scheduler trait - schedulers that support time-based scheduling.
//!
//! A timed scheduler can schedule work at specific time points or after durations.

use crate::traits::Sender;
use std::time::{Duration, Instant};

/// A scheduler that supports time-based scheduling.
///
/// Timed schedulers extend the basic scheduler concept with the ability to
/// schedule work at specific time points or after specific durations.
///
/// # Example
///
/// ```rust,ignore
/// use exec_rs::schedulers::TimedScheduler;
/// use std::time::Duration;
///
/// let scheduler = /* some timed scheduler */;
///
/// // Schedule work to execute after 100ms
/// let sender = scheduler.schedule_after(Duration::from_millis(100));
///
/// // Schedule work at a specific time
/// let when = scheduler.now() + Duration::from_secs(1);
/// let sender = scheduler.schedule_at(when);
/// ```
pub trait TimedScheduler: Clone + PartialEq + Eq {
    /// The type of sender returned by schedule operations.
    type Sender: Sender;

    /// Get the current time according to this scheduler.
    fn now(&self) -> Instant;

    /// Schedule work to execute at a specific time point.
    ///
    /// Returns a sender that completes at or after the specified time.
    fn schedule_at(&self, when: Instant) -> Self::Sender;

    /// Schedule work to execute after a duration.
    ///
    /// This is equivalent to `schedule_at(now() + duration)`.
    fn schedule_after(&self, duration: Duration) -> Self::Sender {
        self.schedule_at(self.now() + duration)
    }

    /// Schedule work to execute as soon as possible.
    ///
    /// This is equivalent to `schedule_at(Instant::now())`.
    fn schedule(&self) -> Self::Sender {
        self.schedule_at(Instant::now())
    }
}
