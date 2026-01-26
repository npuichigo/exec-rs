//! Type-erased scheduler.

use crate::any::{AnyReceiverRef, AnySender};
use crate::traits::{CompletionSignatures, Scheduler, SenderTo};
use std::fmt;
use std::sync::Arc;

/// A type-erased scheduler.
///
/// This allows storing heterogeneous schedulers or returning different
/// scheduler types from functions with a common signature.
///
/// # Example
///
/// ```rust
/// use exec_rs::any::AnyScheduler;
/// use exec_rs::schedulers::InlineScheduler;
/// use exec_rs::traits::Scheduler;
/// use exec_rs::consumers::sync_wait;
///
/// let scheduler: AnyScheduler = AnyScheduler::new(InlineScheduler);
/// let sender = scheduler.schedule();
/// let result = sync_wait(sender).unwrap();
/// assert_eq!(result, ());
/// ```
#[derive(Clone)]
pub struct AnyScheduler {
    inner: Arc<dyn ErasedScheduler + Send + Sync>,
}

impl fmt::Debug for AnyScheduler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnyScheduler").finish_non_exhaustive()
    }
}

impl AnyScheduler {
    /// Create a new type-erased scheduler.
    pub fn new<S>(scheduler: S) -> Self
    where
        S: Scheduler + Send + Sync + 'static,
        S::Sender: Send + 'static,
        <S::Sender as crate::traits::Sender>::Completions:
            CompletionSignatures<Value = (), Error = std::convert::Infallible>,
        S::Sender: for<'a> SenderTo<AnyReceiverRef<'a, (), std::convert::Infallible>>,
        for<'a> <S::Sender as SenderTo<AnyReceiverRef<'a, (), std::convert::Infallible>>>::Operation:
            Send,
    {
        AnyScheduler {
            inner: Arc::new(SchedulerWrapper { scheduler }),
        }
    }
}

impl PartialEq for AnyScheduler {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for AnyScheduler {}

impl Scheduler for AnyScheduler {
    type Sender = AnySender<(), std::convert::Infallible>;

    fn schedule(&self) -> Self::Sender {
        self.inner.schedule_erased()
    }
}

// Internal trait for type erasure
trait ErasedScheduler: Send + Sync {
    fn schedule_erased(&self) -> AnySender<(), std::convert::Infallible>;
}

struct SchedulerWrapper<S> {
    scheduler: S,
}

impl<S> ErasedScheduler for SchedulerWrapper<S>
where
    S: Scheduler + Send + Sync + 'static,
    S::Sender: Send + 'static,
    <S::Sender as crate::traits::Sender>::Completions:
        CompletionSignatures<Value = (), Error = std::convert::Infallible>,
    S::Sender: for<'a> SenderTo<AnyReceiverRef<'a, (), std::convert::Infallible>>,
    for<'a> <S::Sender as SenderTo<AnyReceiverRef<'a, (), std::convert::Infallible>>>::Operation:
        Send,
{
    fn schedule_erased(&self) -> AnySender<(), std::convert::Infallible> {
        AnySender::new(self.scheduler.schedule())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consumers::sync_wait;
    use crate::schedulers::InlineScheduler;

    #[test]
    fn test_any_scheduler_basic() {
        let scheduler: AnyScheduler = AnyScheduler::new(InlineScheduler);
        let sender = scheduler.schedule();
        let result = sync_wait(sender).unwrap();
        assert_eq!(result, ());
    }

    #[test]
    fn test_any_scheduler_clone() {
        let scheduler1: AnyScheduler = AnyScheduler::new(InlineScheduler);
        let scheduler2 = scheduler1.clone();

        // Same Arc, so equal
        assert_eq!(scheduler1, scheduler2);

        let result1 = sync_wait(scheduler1.schedule()).unwrap();
        let result2 = sync_wait(scheduler2.schedule()).unwrap();
        assert_eq!(result1, ());
        assert_eq!(result2, ());
    }

    #[test]
    fn test_any_scheduler_in_function() {
        fn get_scheduler() -> AnyScheduler {
            AnyScheduler::new(InlineScheduler)
        }

        let scheduler = get_scheduler();
        let result = sync_wait(scheduler.schedule()).unwrap();
        assert_eq!(result, ());
    }
}
