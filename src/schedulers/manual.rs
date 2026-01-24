//! Manual scheduler - a scheduler for deterministic testing.
//!
//! The manual scheduler queues work and only executes it when explicitly
//! advanced. This allows for deterministic testing of async behavior.

use crate::traits::{
    CompletionSignatures, OperationState, Receiver, ReceiverOf, ReceiverOfStopped, Scheduler,
    Sender, SenderTo,
};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::pin::Pin;
use std::rc::Rc;

/// A manually-driven scheduler for testing.
///
/// Work scheduled on this scheduler is queued and only executed when
/// `run_one()` or `run_all()` is called. This enables deterministic
/// testing of async behavior.
///
/// # Example
///
/// ```rust
/// use exec_rs::schedulers::ManualScheduler;
/// use exec_rs::traits::Scheduler;
/// use std::sync::atomic::{AtomicUsize, Ordering};
/// use std::sync::Arc;
///
/// let scheduler = ManualScheduler::new();
/// let counter = Arc::new(AtomicUsize::new(0));
///
/// // Schedule some work (doesn't run yet)
/// let counter_clone = counter.clone();
/// scheduler.push(move || {
///     counter_clone.fetch_add(1, Ordering::SeqCst);
/// });
///
/// assert_eq!(counter.load(Ordering::SeqCst), 0);
///
/// // Now run the work
/// scheduler.run_one();
/// assert_eq!(counter.load(Ordering::SeqCst), 1);
/// ```
#[derive(Clone)]
pub struct ManualScheduler {
    inner: Rc<RefCell<ManualSchedulerInner>>,
}

struct ManualSchedulerInner {
    queue: VecDeque<Box<dyn FnOnce()>>,
}

impl std::fmt::Debug for ManualScheduler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ManualScheduler")
            .field("pending", &self.inner.borrow().queue.len())
            .finish()
    }
}

impl ManualScheduler {
    /// Create a new manual scheduler.
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(ManualSchedulerInner {
                queue: VecDeque::new(),
            })),
        }
    }

    /// Push work onto the scheduler's queue.
    pub fn push(&self, work: impl FnOnce() + 'static) {
        self.inner.borrow_mut().queue.push_back(Box::new(work));
    }

    /// Run one queued work item.
    ///
    /// Returns `true` if work was executed, `false` if the queue was empty.
    pub fn run_one(&self) -> bool {
        let work = self.inner.borrow_mut().queue.pop_front();
        if let Some(work) = work {
            work();
            true
        } else {
            false
        }
    }

    /// Run all queued work items.
    ///
    /// Returns the number of work items executed.
    pub fn run_all(&self) -> usize {
        let mut count = 0;
        while self.run_one() {
            count += 1;
        }
        count
    }

    /// Check if there is pending work.
    pub fn has_pending(&self) -> bool {
        !self.inner.borrow().queue.is_empty()
    }

    /// Get the number of pending work items.
    pub fn pending_count(&self) -> usize {
        self.inner.borrow().queue.len()
    }

    /// Clear all pending work without executing it.
    pub fn clear(&self) {
        self.inner.borrow_mut().queue.clear();
    }
}

impl Default for ManualScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialEq for ManualScheduler {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for ManualScheduler {}

impl Scheduler for ManualScheduler {
    type Sender = ManualScheduleSender;

    fn schedule(&self) -> Self::Sender {
        ManualScheduleSender {
            inner: self.inner.clone(),
        }
    }
}

/// Completion signatures for manual schedule sender.
pub struct ManualScheduleCompletions;

impl CompletionSignatures for ManualScheduleCompletions {
    type Value = ();
    type Error = std::convert::Infallible;
    const SENDS_STOPPED: bool = true;
}

/// Sender returned by `ManualScheduler::schedule()`.
#[derive(Clone)]
pub struct ManualScheduleSender {
    inner: Rc<RefCell<ManualSchedulerInner>>,
}

impl Sender for ManualScheduleSender {
    type Completions = ManualScheduleCompletions;
}

impl<R> SenderTo<R> for ManualScheduleSender
where
    R: ReceiverOf<()> + ReceiverOfStopped + 'static,
{
    type Operation = ManualScheduleOperation<R>;

    fn connect(self, receiver: R) -> Self::Operation {
        ManualScheduleOperation {
            inner: self.inner,
            receiver: Some(receiver),
        }
    }
}

/// Operation state for manual schedule sender.
pub struct ManualScheduleOperation<R> {
    inner: Rc<RefCell<ManualSchedulerInner>>,
    receiver: Option<R>,
}

impl<R: ReceiverOf<()> + ReceiverOfStopped + 'static> OperationState
    for ManualScheduleOperation<R>
{
    fn start(self: Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };
        if let Some(receiver) = this.receiver.take() {
            this.inner.borrow_mut().queue.push_back(Box::new(move || {
                // Check stop token
                let env = receiver.get_env();
                let stop_token = crate::traits::Environment::get_stop_token(env);
                if crate::stop::StopToken::stop_requested(&stop_token) {
                    receiver.set_stopped();
                } else {
                    receiver.set_value(());
                }
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adaptors::{on, then};
    use crate::factories::just;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn test_manual_scheduler_basic() {
        let scheduler = ManualScheduler::new();
        let counter = Arc::new(AtomicUsize::new(0));

        let counter_clone = counter.clone();
        scheduler.push(move || {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        });

        assert_eq!(counter.load(Ordering::SeqCst), 0);
        assert!(scheduler.has_pending());
        assert_eq!(scheduler.pending_count(), 1);

        scheduler.run_one();

        assert_eq!(counter.load(Ordering::SeqCst), 1);
        assert!(!scheduler.has_pending());
    }

    #[test]
    fn test_manual_scheduler_run_all() {
        let scheduler = ManualScheduler::new();
        let counter = Arc::new(AtomicUsize::new(0));

        for _ in 0..5 {
            let counter_clone = counter.clone();
            scheduler.push(move || {
                counter_clone.fetch_add(1, Ordering::SeqCst);
            });
        }

        assert_eq!(counter.load(Ordering::SeqCst), 0);
        let count = scheduler.run_all();
        assert_eq!(count, 5);
        assert_eq!(counter.load(Ordering::SeqCst), 5);
    }

    #[test]
    fn test_manual_scheduler_schedule() {
        use crate::consumers::DetachedEnv;
        use std::pin::pin;

        let scheduler = ManualScheduler::new();
        let counter = Arc::new(AtomicUsize::new(0));

        // Create a sender from schedule()
        let sender = scheduler.schedule();

        // Create a simple receiver
        struct TestReceiver {
            counter: Arc<AtomicUsize>,
        }

        impl Receiver for TestReceiver {
            type Env = DetachedEnv;
            fn get_env(&self) -> &Self::Env {
                &DetachedEnv
            }
        }

        impl ReceiverOf<()> for TestReceiver {
            fn set_value(self, _: ()) {
                self.counter.fetch_add(1, Ordering::SeqCst);
            }
        }

        impl ReceiverOfStopped for TestReceiver {
            fn set_stopped(self) {}
        }

        let receiver = TestReceiver {
            counter: counter.clone(),
        };

        let mut op = pin!(sender.connect(receiver));
        op.as_mut().start();

        assert_eq!(counter.load(Ordering::SeqCst), 0);
        assert!(scheduler.has_pending());

        scheduler.run_one();

        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_manual_scheduler_clear() {
        let scheduler = ManualScheduler::new();

        for _ in 0..5 {
            scheduler.push(|| {});
        }

        assert_eq!(scheduler.pending_count(), 5);
        scheduler.clear();
        assert_eq!(scheduler.pending_count(), 0);
    }

    #[test]
    fn test_manual_scheduler_equality() {
        let s1 = ManualScheduler::new();
        let s2 = s1.clone();
        let s3 = ManualScheduler::new();

        assert_eq!(s1, s2);
        assert_ne!(s1, s3);
    }
}
