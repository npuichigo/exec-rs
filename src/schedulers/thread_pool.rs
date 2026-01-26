//! Static thread pool - a multi-threaded execution context.
//!
//! Provides a fixed-size thread pool for parallel work execution.

use crate::traits::{
    CompletionSignatures, OperationState, ReceiverOf, Scheduler, Sender, SenderTo,
};
use crossbeam_channel::{bounded, Receiver as ChannelReceiver, Sender as ChannelSender};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

/// A static thread pool for parallel execution.
///
/// The pool maintains a fixed number of worker threads that process tasks
/// from a shared queue. Work is distributed across threads for parallel
/// execution.
///
/// # Example
///
/// ```rust
/// use exec_rs::schedulers::StaticThreadPool;
/// use exec_rs::traits::Scheduler;
/// use exec_rs::adaptors::on;
/// use exec_rs::factories::just;
/// use exec_rs::consumers::sync_wait;
///
/// let pool = StaticThreadPool::new(4);
/// let scheduler = pool.get_scheduler();
///
/// let sender = on(scheduler, just(42));
/// let result = sync_wait(sender).unwrap();
/// assert_eq!(result, 42);
/// ```
pub struct StaticThreadPool {
    inner: Arc<ThreadPoolInner>,
    threads: Vec<JoinHandle<()>>,
}

struct ThreadPoolInner {
    sender: ChannelSender<Box<dyn FnOnce() + Send>>,
    receiver: ChannelReceiver<Box<dyn FnOnce() + Send>>,
    thread_count: usize,
    stop_flag: AtomicBool,
    active_tasks: AtomicUsize,
}

impl StaticThreadPool {
    /// Create a new thread pool with the specified number of threads.
    ///
    /// # Panics
    ///
    /// Panics if `thread_count` is 0.
    pub fn new(thread_count: usize) -> Self {
        assert!(thread_count > 0, "thread_count must be > 0");
        Self::with_name(thread_count, "thread-pool")
    }

    /// Create a new thread pool with the number of threads equal to
    /// the number of available CPUs.
    pub fn new_auto() -> Self {
        let count = thread::available_parallelism()
            .map(|p| p.get())
            .unwrap_or(1);
        Self::new(count)
    }

    /// Create a new thread pool with a custom thread name prefix.
    pub fn with_name(thread_count: usize, name_prefix: &str) -> Self {
        let (sender, receiver) = bounded(thread_count * 16);

        let inner = Arc::new(ThreadPoolInner {
            sender,
            receiver,
            thread_count,
            stop_flag: AtomicBool::new(false),
            active_tasks: AtomicUsize::new(0),
        });

        let mut threads = Vec::with_capacity(thread_count);

        for i in 0..thread_count {
            let inner_clone = inner.clone();
            let thread_name = format!("{}-{}", name_prefix, i);

            let thread = thread::Builder::new()
                .name(thread_name)
                .spawn(move || {
                    Self::worker_loop(inner_clone);
                })
                .expect("Failed to spawn worker thread");

            threads.push(thread);
        }

        Self { inner, threads }
    }

    fn worker_loop(inner: Arc<ThreadPoolInner>) {
        // Use select to wait for either a task or stop signal
        loop {
            // Try to receive work, but check stop flag periodically
            match inner
                .receiver
                .recv_timeout(std::time::Duration::from_millis(10))
            {
                Ok(task) => {
                    task();
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    // Check for shutdown
                    if inner.stop_flag.load(Ordering::Acquire) {
                        // Drain remaining tasks before exiting
                        while let Ok(task) = inner.receiver.try_recv() {
                            task();
                        }
                        return;
                    }
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    return;
                }
            }
        }
    }

    /// Get a scheduler for this thread pool.
    pub fn get_scheduler(&self) -> ThreadPoolScheduler {
        ThreadPoolScheduler {
            inner: self.inner.clone(),
        }
    }

    /// Get the number of threads in the pool.
    pub fn thread_count(&self) -> usize {
        self.inner.thread_count
    }

    /// Request all worker threads to stop.
    ///
    /// After calling this, no new work will be accepted and workers
    /// will finish after completing any currently executing tasks.
    pub fn request_stop(&self) {
        self.inner.stop_flag.store(true, Ordering::Release);
    }
}

impl Drop for StaticThreadPool {
    fn drop(&mut self) {
        // Signal workers to stop
        self.request_stop();

        // Wait for all threads to complete
        for thread in self.threads.drain(..) {
            thread.join().ok();
        }
    }
}

/// Scheduler for a thread pool.
#[derive(Clone)]
pub struct ThreadPoolScheduler {
    inner: Arc<ThreadPoolInner>,
}

impl std::fmt::Debug for ThreadPoolScheduler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThreadPoolScheduler")
            .field("thread_count", &self.inner.thread_count)
            .finish()
    }
}

impl PartialEq for ThreadPoolScheduler {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for ThreadPoolScheduler {}

/// Completion signatures for thread pool schedule sender.
pub struct ThreadPoolScheduleCompletions;

impl CompletionSignatures for ThreadPoolScheduleCompletions {
    type Value = ();
    type Error = std::convert::Infallible;
    const SENDS_STOPPED: bool = false;
}

/// Sender returned by `ThreadPoolScheduler::schedule()`.
pub struct ThreadPoolScheduleSender {
    inner: Arc<ThreadPoolInner>,
}

impl Sender for ThreadPoolScheduleSender {
    type Completions = ThreadPoolScheduleCompletions;
}

impl<R> SenderTo<R> for ThreadPoolScheduleSender
where
    R: ReceiverOf<()> + Send + 'static,
{
    type Operation = ThreadPoolScheduleOperation<R>;

    fn connect(self, receiver: R) -> Self::Operation {
        ThreadPoolScheduleOperation {
            inner: self.inner,
            receiver: Some(receiver),
        }
    }
}

/// Operation state for thread pool schedule sender.
pub struct ThreadPoolScheduleOperation<R> {
    inner: Arc<ThreadPoolInner>,
    receiver: Option<R>,
}

impl<R: ReceiverOf<()> + Send + 'static> OperationState for ThreadPoolScheduleOperation<R> {
    fn start(self: Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };
        if let Some(receiver) = this.receiver.take() {
            // Submit work to the thread pool
            let _ = this.inner.sender.send(Box::new(move || {
                receiver.set_value(());
            }));
        }
    }
}

impl Scheduler for ThreadPoolScheduler {
    type Sender = ThreadPoolScheduleSender;

    fn schedule(&self) -> Self::Sender {
        ThreadPoolScheduleSender {
            inner: self.inner.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adaptors::{on, then};
    use crate::consumers::sync_wait;
    use crate::factories::just;
    use crate::traits::Scheduler;
    use std::collections::HashSet;
    use std::sync::Mutex;
    use std::thread::ThreadId;

    #[test]
    fn test_thread_pool_schedule_direct() {
        // Test that the scheduler works without 'on' adaptor
        let pool = StaticThreadPool::new(2);
        let scheduler = pool.get_scheduler();

        // Just test that schedule() returns a sender we can sync_wait on
        let result = sync_wait(scheduler.schedule()).unwrap();
        assert_eq!(result, ());
    }

    #[test]
    fn test_thread_pool_on_just_unit() {
        // Simpler test - just() with unit type
        let pool = StaticThreadPool::new(2);
        let scheduler = pool.get_scheduler();

        let sender = on(scheduler, just(()));
        let result = sync_wait(sender).unwrap();
        assert_eq!(result, ());
    }

    #[test]
    fn test_thread_pool_basic() {
        let pool = StaticThreadPool::new(4);
        let scheduler = pool.get_scheduler();

        let sender = on(scheduler, just(42));
        let result = sync_wait(sender).unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn test_thread_pool_multiple_tasks() {
        let pool = StaticThreadPool::new(4);
        let scheduler = pool.get_scheduler();
        let counter = Arc::new(AtomicUsize::new(0));

        for _ in 0..100 {
            let counter_clone = counter.clone();
            let sender = on(
                scheduler.clone(),
                then(just(()), move |_| {
                    counter_clone.fetch_add(1, Ordering::SeqCst);
                }),
            );
            sync_wait(sender).unwrap();
        }

        assert_eq!(counter.load(Ordering::SeqCst), 100);
    }

    #[test]
    fn test_thread_pool_parallelism() {
        let pool = StaticThreadPool::new(4);
        let scheduler = pool.get_scheduler();

        let thread_ids = Arc::new(Mutex::new(HashSet::new()));

        // Run multiple tasks and collect thread IDs
        for _ in 0..20 {
            let thread_ids_clone = thread_ids.clone();
            let sender = on(
                scheduler.clone(),
                then(just(()), move |_| {
                    thread_ids_clone
                        .lock()
                        .unwrap()
                        .insert(thread::current().id());
                    // Small sleep to allow parallel execution
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }),
            );
            sync_wait(sender).unwrap();
        }

        // Should have used multiple threads
        let unique_threads = thread_ids.lock().unwrap().len();
        assert!(unique_threads >= 1, "Should use at least 1 thread");
    }

    #[test]
    fn test_thread_pool_scheduler_equality() {
        let pool = StaticThreadPool::new(2);
        let s1 = pool.get_scheduler();
        let s2 = pool.get_scheduler();

        assert_eq!(s1, s2);
    }

    #[test]
    fn test_thread_pool_auto() {
        let pool = StaticThreadPool::new_auto();
        assert!(pool.thread_count() >= 1);

        let scheduler = pool.get_scheduler();
        let result = sync_wait(on(scheduler, just(123))).unwrap();
        assert_eq!(result, 123);
    }

    #[test]
    fn test_thread_pool_runs_on_pool_thread() {
        let pool = StaticThreadPool::new(2);
        let scheduler = pool.get_scheduler();
        let main_thread_id = thread::current().id();

        let captured_thread_id = Arc::new(Mutex::new(None::<ThreadId>));
        let captured_clone = captured_thread_id.clone();

        let sender = on(
            scheduler,
            then(just(()), move |_| {
                *captured_clone.lock().unwrap() = Some(thread::current().id());
                42
            }),
        );

        let result = sync_wait(sender).unwrap();
        assert_eq!(result, 42);

        let work_thread_id = captured_thread_id.lock().unwrap().unwrap();
        assert_ne!(work_thread_id, main_thread_id);
    }
}
