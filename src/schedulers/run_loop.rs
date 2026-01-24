//! Run loop - a single-threaded event loop scheduler.
//!
//! The run loop maintains a queue of work items and processes them
//! sequentially. It's used by `sync_wait` to drive sender execution.

use crate::traits::{
    CompletionSignatures, OperationState, Receiver, ReceiverOf, Scheduler, Sender, SenderTo,
    SingleThreadedScheduler,
};
use parking_lot::{Condvar, Mutex};
use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Arc;

/// A single-threaded event loop.
///
/// The run loop maintains a FIFO queue of work items. When `run()` is called,
/// it processes items until `finish()` is called.
///
/// # Example
///
/// ```rust
/// use exec_rs::schedulers::RunLoop;
/// use exec_rs::traits::{Scheduler, SenderTo, OperationState};
/// use std::pin::Pin;
///
/// let run_loop = RunLoop::new();
/// let scheduler = run_loop.get_scheduler();
///
/// // Schedule some work
/// // ... (would need to spawn a thread to call run_loop.run())
/// ```
pub struct RunLoop {
    inner: Arc<RunLoopInner>,
}

/// State protected by the mutex - includes both queue and finished flag
/// to ensure proper synchronization with condvar.
pub(crate) struct RunLoopState {
    pub(crate) queue: VecDeque<Box<dyn FnOnce() + Send>>,
    pub(crate) finished: bool,
}

pub(crate) struct RunLoopInner {
    pub(crate) state: Mutex<RunLoopState>,
    pub(crate) condvar: Condvar,
}

impl std::fmt::Debug for RunLoopInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state.lock();
        f.debug_struct("RunLoopInner")
            .field("queue", &format!("<{} items>", state.queue.len()))
            .field("finished", &state.finished)
            .finish()
    }
}

impl RunLoop {
    /// Create a new run loop.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RunLoopInner {
                state: Mutex::new(RunLoopState {
                    queue: VecDeque::new(),
                    finished: false,
                }),
                condvar: Condvar::new(),
            }),
        }
    }

    /// Get a scheduler for this run loop.
    pub fn get_scheduler(&self) -> RunLoopScheduler {
        RunLoopScheduler {
            inner: self.inner.clone(),
        }
    }

    /// Run the event loop until `finish()` is called.
    ///
    /// This blocks the current thread, processing work items from the queue
    /// until `finish()` is called.
    pub fn run(&self) {
        loop {
            let work = {
                let mut state = self.inner.state.lock();

                // Wait while queue is empty AND not finished.
                // This is the proper condvar pattern - the predicate is checked
                // while holding the lock, and wait_while atomically releases
                // the lock and waits, then re-acquires before returning.
                self.inner
                    .condvar
                    .wait_while(&mut state, |s| s.queue.is_empty() && !s.finished);

                // Check if we should exit (finished and queue drained)
                if state.finished && state.queue.is_empty() {
                    return;
                }

                state.queue.pop_front()
            };

            if let Some(work) = work {
                work();
            }
        }
    }

    /// Signal that the run loop should finish.
    ///
    /// After this is called, `run()` will return once all pending work is
    /// complete.
    pub fn finish(&self) {
        {
            let mut state = self.inner.state.lock();
            state.finished = true;
        }
        self.inner.condvar.notify_all();
    }

    /// Push work onto the queue.
    pub fn push(&self, work: impl FnOnce() + Send + 'static) {
        {
            let mut state = self.inner.state.lock();
            state.queue.push_back(Box::new(work));
        }
        self.inner.condvar.notify_one();
    }

    /// Check if the run loop has been signaled to finish.
    pub fn is_finished(&self) -> bool {
        self.inner.state.lock().finished
    }
}

impl Default for RunLoop {
    fn default() -> Self {
        Self::new()
    }
}

/// Scheduler for a run loop.
#[derive(Clone, Debug)]
pub struct RunLoopScheduler {
    pub(crate) inner: Arc<RunLoopInner>,
}

impl PartialEq for RunLoopScheduler {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for RunLoopScheduler {}

impl Scheduler for RunLoopScheduler {
    type Sender = RunLoopScheduleSender;

    fn schedule(&self) -> Self::Sender {
        RunLoopScheduleSender {
            inner: self.inner.clone(),
        }
    }
}

impl SingleThreadedScheduler for RunLoopScheduler {}

/// Completion signatures for run loop schedule sender.
pub struct RunLoopScheduleCompletions;

impl CompletionSignatures for RunLoopScheduleCompletions {
    type Value = ();
    type Error = std::convert::Infallible;
    const SENDS_STOPPED: bool = false;
}

/// Sender returned by `RunLoopScheduler::schedule()`.
pub struct RunLoopScheduleSender {
    inner: Arc<RunLoopInner>,
}

impl Sender for RunLoopScheduleSender {
    type Completions = RunLoopScheduleCompletions;
}

impl<R> SenderTo<R> for RunLoopScheduleSender
where
    R: ReceiverOf<()> + Send + 'static,
{
    type Operation = RunLoopScheduleOperation<R>;

    fn connect(self, receiver: R) -> Self::Operation {
        RunLoopScheduleOperation {
            inner: self.inner,
            receiver: Some(receiver),
        }
    }
}

/// Operation state for run loop schedule sender.
pub struct RunLoopScheduleOperation<R> {
    inner: Arc<RunLoopInner>,
    receiver: Option<R>,
}

impl<R: ReceiverOf<()> + Send + 'static> OperationState for RunLoopScheduleOperation<R> {
    fn start(self: Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };
        if let Some(receiver) = this.receiver.take() {
            // Push work to the run loop
            {
                let mut state = this.inner.state.lock();
                state.queue.push_back(Box::new(move || {
                    receiver.set_value(());
                }));
            }
            this.inner.condvar.notify_one();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

    #[test]
    fn test_run_loop_basic() {
        let run_loop = RunLoop::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        run_loop.push(move || {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        });

        run_loop.finish();
        run_loop.run();

        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_run_loop_multiple_items() {
        let run_loop = RunLoop::new();
        let counter = Arc::new(AtomicUsize::new(0));

        for _ in 0..10 {
            let counter_clone = counter.clone();
            run_loop.push(move || {
                counter_clone.fetch_add(1, Ordering::SeqCst);
            });
        }

        run_loop.finish();
        run_loop.run();

        assert_eq!(counter.load(Ordering::SeqCst), 10);
    }

    #[test]
    fn test_run_loop_scheduler() {
        let run_loop = RunLoop::new();
        let s1 = run_loop.get_scheduler();
        let s2 = run_loop.get_scheduler();

        assert_eq!(s1, s2);
    }
}
