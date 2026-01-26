//! Run loop - a single-threaded event loop scheduler.
//!
//! The run loop maintains a queue of work items and processes them
//! sequentially. It's used by `sync_wait` to drive sender execution.
//!
//! This implementation uses a lock-free atomic intrusive queue for task
//! submission (MPSC pattern) with atomic wait/notify for consumer blocking.

use crate::traits::{
    CompletionSignatures, OperationState, ReceiverOf, Scheduler, Sender, SenderTo,
    SingleThreadedScheduler,
};
use crate::util::{AtomicIntrusiveQueue, IntrusiveNode};
use std::cell::Cell;
use std::pin::Pin;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

/// A single-threaded event loop.
///
/// The run loop maintains a FIFO queue of work items. When `run()` is called,
/// it processes items until `finish()` is called.
///
/// This implementation uses a lock-free intrusive queue for task submission,
/// providing better performance than mutex-protected VecDeque for high-throughput
/// scenarios.
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

/// Task base type for intrusive queue.
///
/// Each task contains its own next pointer and a function pointer for execution.
pub struct Task {
    /// Function to execute this task. Takes the task pointer as argument.
    execute_fn: fn(*mut Task),
    /// Intrusive next pointer for queue linkage.
    next: Cell<Option<NonNull<Task>>>,
}

// SAFETY: Tasks are designed for multi-threaded submission
unsafe impl Send for Task {}
unsafe impl Sync for Task {}

unsafe impl IntrusiveNode for Task {
    fn next(&self) -> &Cell<Option<NonNull<Self>>> {
        &self.next
    }
}

impl Task {
    /// Create a new task with the given execute function.
    pub fn new(execute_fn: fn(*mut Task)) -> Self {
        Self {
            execute_fn,
            next: Cell::new(None),
        }
    }

    /// Execute this task.
    ///
    /// # Safety
    ///
    /// The task pointer must be valid.
    pub unsafe fn execute(&self, task_ptr: *mut Task) {
        (self.execute_fn)(task_ptr);
    }
}

/// A boxed task that owns a closure.
#[repr(C)]
struct BoxedTask<F: FnOnce() + Send + 'static> {
    base: Task,
    func: Option<F>,
}

impl<F: FnOnce() + Send + 'static> BoxedTask<F> {
    fn new(func: F) -> Box<Self> {
        Box::new(Self {
            base: Task::new(Self::execute_impl),
            func: Some(func),
        })
    }

    fn execute_impl(task_ptr: *mut Task) {
        // SAFETY: We know this is a BoxedTask<F>
        let boxed = unsafe { Box::from_raw(task_ptr as *mut BoxedTask<F>) };
        if let Some(func) = boxed.func {
            func();
        }
        // boxed is dropped here, freeing memory
    }
}

pub(crate) struct RunLoopInner {
    /// Lock-free task queue (MPSC) with atomic wait/notify.
    queue: AtomicIntrusiveQueue<Task>,
    /// Task count for proper shutdown.
    task_count: AtomicUsize,
    /// Flag indicating finish has been requested.
    finishing: AtomicBool,
    /// Noop task used to wake consumer on finish.
    noop_task: Task,
}

impl std::fmt::Debug for RunLoopInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunLoopInner")
            .field("task_count", &self.task_count.load(Ordering::Relaxed))
            .field("finishing", &self.finishing.load(Ordering::Relaxed))
            .finish()
    }
}

impl RunLoop {
    /// Create a new run loop.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RunLoopInner {
                queue: AtomicIntrusiveQueue::new(),
                task_count: AtomicUsize::new(0),
                finishing: AtomicBool::new(false),
                noop_task: Task::new(|_| {}), // No-op
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
    /// until `finish()` is called and all pending work is complete.
    pub fn run(&self) {
        // Execute work items until the finishing flag is set
        while !self.inner.finishing.load(Ordering::Acquire) {
            self.wait_for_item();
            self.execute_all();
        }

        // Drain the queue, processing any tasks that get added while executing
        // remaining tasks (also wait for tasks that might still be in flight)
        while self.execute_all() || self.inner.task_count.load(Ordering::Acquire) > 0 {
            // Keep processing
        }
    }

    /// Wait for an item to be available in the queue.
    fn wait_for_item(&self) {
        // Fast path: check if queue has items or we're finishing
        if !self.inner.queue.is_empty() || self.inner.finishing.load(Ordering::Acquire) {
            return;
        }

        // Use atomic wait - this is race-free because the check and wait are atomic.
        // If the queue becomes non-empty between our check and the wait call,
        // the wait will return immediately.
        self.inner.queue.wait_for_item();
    }

    /// Execute all tasks currently in the queue.
    ///
    /// Returns true if any tasks were executed.
    fn execute_all(&self) -> bool {
        // Dequeue all tasks at once
        let mut queue = self.inner.queue.pop_all();

        if queue.is_empty() {
            return false;
        }

        let mut task_count = 0;

        // Execute all tasks
        while let Some(task_ptr) = queue.pop_front() {
            // SAFETY: Task pointer is valid - it was pushed to the queue
            unsafe {
                let task = task_ptr.as_ref();
                task.execute(task_ptr.as_ptr());
            }
            task_count += 1;
        }

        self.inner
            .task_count
            .fetch_sub(task_count, Ordering::Release);
        true
    }

    /// Signal that the run loop should finish.
    ///
    /// After this is called, `run()` will return once all pending work is
    /// complete.
    pub fn finish(&self) {
        // Increment task count by 2 to prevent lifetime issues if finish
        // is called from a different thread
        self.inner.task_count.fetch_add(2, Ordering::Release);

        if !self.inner.finishing.swap(true, Ordering::AcqRel) {
            // Push a noop task to wake up the consumer.
            // The push() method automatically calls wake_one() if the queue was empty.
            // SAFETY: noop_task is owned by inner and outlives the run loop
            unsafe {
                let noop_ptr = &self.inner.noop_task as *const Task as *mut Task;
                self.inner.queue.push(NonNull::new_unchecked(noop_ptr));
            }
            // Decrement by 1 (the other decrement happens implicitly)
            self.inner.task_count.fetch_sub(1, Ordering::Release);
            return;
        }

        // Already finishing, decrement by 2
        self.inner.task_count.fetch_sub(2, Ordering::Release);
    }

    /// Push work onto the queue.
    ///
    /// This is a convenience method that boxes the closure. For zero-allocation
    /// task submission, use the scheduler's connect/start pattern.
    pub fn push(&self, work: impl FnOnce() + Send + 'static) {
        let task = BoxedTask::new(work);
        let task_ptr = Box::into_raw(task) as *mut Task;

        self.inner.task_count.fetch_add(1, Ordering::Release);

        // SAFETY: task_ptr is valid, allocated via Box
        // push() automatically wakes the consumer if the queue was empty
        unsafe { self.inner.queue.push(NonNull::new_unchecked(task_ptr)) };
    }

    /// Check if the run loop has been signaled to finish.
    pub fn is_finished(&self) -> bool {
        self.inner.finishing.load(Ordering::Acquire)
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
        RunLoopScheduleOperation::new(self.inner, receiver)
    }
}

/// Operation state for run loop schedule sender.
///
/// This embeds the Task directly to avoid allocation when scheduling.
/// The operation state IS the task (first field), allowing zero-copy
/// submission to the run loop queue.
#[repr(C)]
pub struct RunLoopScheduleOperation<R> {
    /// Embedded task - MUST be first field for pointer casting to work
    task: Task,
    /// The run loop to schedule on
    inner: Arc<RunLoopInner>,
    /// The receiver to complete
    receiver: Option<R>,
}

impl<R: ReceiverOf<()> + Send + 'static> RunLoopScheduleOperation<R> {
    fn new(inner: Arc<RunLoopInner>, receiver: R) -> Self {
        Self {
            task: Task::new(Self::execute_impl),
            inner,
            receiver: Some(receiver),
        }
    }

    /// Execute function called by the run loop.
    ///
    /// This casts the task pointer back to the operation and completes the receiver.
    fn execute_impl(task_ptr: *mut Task) {
        // SAFETY: task_ptr points to the `task` field of a RunLoopScheduleOperation<R>
        // which is repr(C) with task as the first field
        let op_ptr = task_ptr as *mut RunLoopScheduleOperation<R>;
        unsafe {
            if let Some(receiver) = (*op_ptr).receiver.take() {
                receiver.set_value(());
            }
        }
    }
}

impl<R: ReceiverOf<()> + Send + 'static> OperationState for RunLoopScheduleOperation<R> {
    fn start(self: Pin<&mut Self>) {
        // SAFETY: We only access fields, not move the operation
        let this = unsafe { self.get_unchecked_mut() };

        // Get pointer to embedded task (which is self due to repr(C))
        let task_ptr = &mut this.task as *mut Task;

        this.inner.task_count.fetch_add(1, Ordering::Release);

        // SAFETY: task_ptr is valid and points to pinned memory
        // push() automatically wakes the consumer if the queue was empty
        unsafe { this.inner.queue.push(NonNull::new_unchecked(task_ptr)) };
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

    #[test]
    fn test_run_loop_concurrent_push() {
        let run_loop = Arc::new(RunLoop::new());
        let counter = Arc::new(AtomicUsize::new(0));
        let num_threads = 4;
        let items_per_thread = 100;

        let mut handles = vec![];

        for _ in 0..num_threads {
            let run_loop = run_loop.clone();
            let counter = counter.clone();
            handles.push(thread::spawn(move || {
                for _ in 0..items_per_thread {
                    let counter = counter.clone();
                    run_loop.push(move || {
                        counter.fetch_add(1, Ordering::SeqCst);
                    });
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        run_loop.finish();
        run_loop.run();

        assert_eq!(
            counter.load(Ordering::SeqCst),
            num_threads * items_per_thread
        );
    }

    #[test]
    fn test_run_loop_finish_while_running() {
        let run_loop = Arc::new(RunLoop::new());
        let counter = Arc::new(AtomicUsize::new(0));

        // Spawn producer thread
        let run_loop_clone = run_loop.clone();
        let counter_clone = counter.clone();
        let producer = thread::spawn(move || {
            for _ in 0..100 {
                let counter = counter_clone.clone();
                run_loop_clone.push(move || {
                    counter.fetch_add(1, Ordering::SeqCst);
                });
            }
            run_loop_clone.finish();
        });

        // Run on main thread
        run_loop.run();

        producer.join().unwrap();

        assert_eq!(counter.load(Ordering::SeqCst), 100);
    }

    #[test]
    fn test_run_loop_worker_thread_pattern() {
        // Simulate SingleThreadContext pattern
        let run_loop = Arc::new(RunLoop::new());
        let run_loop_for_worker = run_loop.clone();

        let worker = thread::spawn(move || {
            run_loop_for_worker.run();
        });

        // Push some work
        let counter = Arc::new(AtomicUsize::new(0));
        for _ in 0..10 {
            let counter = counter.clone();
            run_loop.push(move || {
                counter.fetch_add(1, Ordering::SeqCst);
            });
        }

        // Give worker time to process
        thread::sleep(std::time::Duration::from_millis(10));

        // Signal finish
        run_loop.finish();

        // Wait for worker
        worker.join().unwrap();

        assert_eq!(counter.load(Ordering::SeqCst), 10);
    }
}
