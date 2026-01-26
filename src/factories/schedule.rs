//! schedule - A factory that creates a sender from a scheduler.
//!
//! The `schedule` function is a Customization Point Object (CPO) that takes
//! a scheduler and returns a sender that completes on that scheduler's
//! execution context.

use crate::traits::Scheduler;

/// Creates a sender that completes on the given scheduler's execution context.
///
/// This is the primary way to enter a scheduler's execution context.
/// The returned sender completes with `()` when executed on the scheduler.
///
/// # Example
///
/// ```rust
/// use exec_rs::factories::schedule;
/// use exec_rs::schedulers::InlineScheduler;
/// use exec_rs::consumers::sync_wait;
/// use exec_rs::adaptors::then;
///
/// let scheduler = InlineScheduler;
/// let result = sync_wait(then(schedule(scheduler), |_| 42)).unwrap();
/// assert_eq!(result, 42);
/// ```
pub fn schedule<S: Scheduler>(scheduler: S) -> S::Sender {
    scheduler.schedule()
}

/// Creates a sender that completes with a value on the given scheduler.
///
/// This is equivalent to `then(schedule(scheduler), |_| value)` but
/// more convenient for simple cases.
///
/// # Example
///
/// ```rust
/// use exec_rs::factories::transfer_just;
/// use exec_rs::schedulers::InlineScheduler;
/// use exec_rs::consumers::sync_wait;
///
/// let scheduler = InlineScheduler;
/// let result = sync_wait(transfer_just(scheduler, 42)).unwrap();
/// assert_eq!(result, 42);
/// ```
pub fn transfer_just<S, T>(
    scheduler: S,
    value: T,
) -> crate::adaptors::Then<S::Sender, impl FnOnce(()) -> T>
where
    S: Scheduler,
    T: 'static,
{
    crate::adaptors::then(schedule(scheduler), move |_| value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adaptors::then;
    use crate::consumers::sync_wait;
    use crate::schedulers::InlineScheduler;

    #[test]
    fn test_schedule_inline() {
        let scheduler = InlineScheduler;
        let sender = schedule(scheduler);
        let result = sync_wait(then(sender, |_| 42)).unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn test_schedule_completes_with_unit() {
        let scheduler = InlineScheduler;
        let sender = schedule(scheduler);
        let result = sync_wait(sender).unwrap();
        assert_eq!(result, ());
    }

    #[test]
    fn test_transfer_just() {
        let scheduler = InlineScheduler;
        let result = sync_wait(transfer_just(scheduler, 42)).unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn test_transfer_just_string() {
        let scheduler = InlineScheduler;
        let result = sync_wait(transfer_just(scheduler, "hello")).unwrap();
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_schedule_with_thread_pool() {
        use crate::schedulers::StaticThreadPool;
        use std::thread;

        let pool = StaticThreadPool::new(2);
        let main_thread = thread::current().id();

        // Schedule work on the thread pool
        let sender = then(schedule(pool.get_scheduler()), move |_| {
            // This should run on a pool thread, not the main thread
            thread::current().id() != main_thread
        });

        let ran_on_pool = sync_wait(sender).unwrap();
        assert!(ran_on_pool, "Work should run on thread pool");
        // Pool is dropped here, cleaning up threads
    }
}
