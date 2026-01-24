//! Single thread context - a dedicated thread with a run loop.
//!
//! This provides a single-threaded execution context that runs on a dedicated
//! background thread, similar to stdexec's `single_thread_context`.

use super::run_loop::{RunLoop, RunLoopScheduler};
use std::thread::{self, JoinHandle, ThreadId};

/// A single-threaded execution context.
///
/// Creates a dedicated background thread that runs a `RunLoop`. Work scheduled
/// on this context's scheduler will execute on the dedicated thread.
///
/// # Example
///
/// ```rust
/// use exec_rs::schedulers::SingleThreadContext;
/// use exec_rs::traits::Scheduler;
/// use exec_rs::factories::just;
/// use exec_rs::adaptors::on;
/// use exec_rs::consumers::sync_wait;
///
/// let ctx = SingleThreadContext::new();
/// let scheduler = ctx.get_scheduler();
///
/// // Work scheduled here runs on the dedicated thread
/// let sender = on(scheduler, just(42));
/// let result = sync_wait(sender).unwrap();
/// assert_eq!(result, 42);
/// // ctx is dropped here, stopping the thread
/// ```
pub struct SingleThreadContext {
    run_loop: RunLoop,
    thread: Option<JoinHandle<()>>,
    thread_id: ThreadId,
}

impl SingleThreadContext {
    /// Create a new single-thread context with a default thread name.
    pub fn new() -> Self {
        Self::with_name("single-thread-context")
    }

    /// Create a new single-thread context with a custom thread name.
    pub fn with_name(name: &str) -> Self {
        let run_loop = RunLoop::new();
        let run_loop_for_thread = run_loop.get_scheduler();

        let thread = thread::Builder::new()
            .name(name.to_string())
            .spawn(move || {
                // Get the inner run loop and run it
                // We need to run until finish() is called
                loop {
                    let work = {
                        let mut state = run_loop_for_thread.inner.state.lock();

                        // Wait while queue is empty AND not finished.
                        // This is the proper condvar pattern - the predicate is checked
                        // while holding the lock, and wait_while atomically releases
                        // the lock and waits, then re-acquires before returning.
                        run_loop_for_thread
                            .inner
                            .condvar
                            .wait_while(&mut state, |s| s.queue.is_empty() && !s.finished);

                        if state.finished && state.queue.is_empty() {
                            return;
                        }

                        state.queue.pop_front()
                    };

                    if let Some(work) = work {
                        work();
                    }
                }
            })
            .expect("Failed to spawn thread");

        let thread_id = thread.thread().id();

        Self {
            run_loop,
            thread: Some(thread),
            thread_id,
        }
    }

    /// Get a scheduler for this context.
    ///
    /// Work scheduled on this scheduler will execute on the dedicated thread.
    pub fn get_scheduler(&self) -> RunLoopScheduler {
        self.run_loop.get_scheduler()
    }

    /// Get the thread ID of the dedicated thread.
    pub fn get_thread_id(&self) -> ThreadId {
        self.thread_id
    }

    /// Check if the current thread is the context's thread.
    pub fn is_on_context(&self) -> bool {
        thread::current().id() == self.thread_id
    }
}

impl Default for SingleThreadContext {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for SingleThreadContext {
    fn drop(&mut self) {
        // Signal the run loop to finish
        self.run_loop.finish();

        // Wait for the thread to complete
        if let Some(thread) = self.thread.take() {
            thread.join().ok();
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::thread::ThreadId;

    #[test]
    fn test_single_thread_context_basic() {
        let ctx = SingleThreadContext::new();
        let scheduler = ctx.get_scheduler();

        let sender = on(scheduler, just(42));
        let result = sync_wait(sender).unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn test_single_thread_context_runs_on_dedicated_thread() {
        let ctx = SingleThreadContext::new();
        let ctx_thread_id = ctx.get_thread_id();
        let scheduler = ctx.get_scheduler();

        let captured_thread_id = Arc::new(std::sync::Mutex::new(None::<ThreadId>));
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
        assert_eq!(work_thread_id, ctx_thread_id);
        assert_ne!(work_thread_id, thread::current().id());
    }

    #[test]
    fn test_single_thread_context_multiple_tasks() {
        let ctx = SingleThreadContext::new();
        let scheduler = ctx.get_scheduler();
        let counter = Arc::new(AtomicUsize::new(0));

        for i in 0..10 {
            let counter_clone = counter.clone();
            let sender = on(
                scheduler.clone(),
                then(just(i), move |_| {
                    counter_clone.fetch_add(1, Ordering::SeqCst);
                }),
            );
            sync_wait(sender).unwrap();
        }

        assert_eq!(counter.load(Ordering::SeqCst), 10);
    }

    #[test]
    fn test_single_thread_context_with_name() {
        let ctx = SingleThreadContext::with_name("my-custom-thread");
        let scheduler = ctx.get_scheduler();

        let result = sync_wait(on(scheduler, just(123))).unwrap();
        assert_eq!(result, 123);
    }
}
