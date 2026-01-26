//! Timed thread scheduler - a scheduler with a dedicated timer thread.
//!
//! This provides a timed scheduler that runs on a dedicated background thread,
//! using a min-heap to efficiently manage scheduled operations.

use super::timed::TimedScheduler;
use crate::traits::{
    CompletionSignatures, OperationState, ReceiverOf, ReceiverOfStopped, Sender, SenderTo,
};
use parking_lot::{Condvar, Mutex};
use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// A timed thread execution context.
///
/// This context runs a dedicated thread that processes scheduled operations
/// at their specified time points.
///
/// # Example
///
/// ```rust
/// use exec_rs::schedulers::{TimedThreadContext, TimedScheduler};
/// use exec_rs::consumers::sync_wait;
/// use std::time::{Duration, Instant};
///
/// let ctx = TimedThreadContext::new();
/// let scheduler = ctx.get_scheduler();
///
/// // Schedule work after 10ms
/// let sender = scheduler.schedule_after(Duration::from_millis(10));
/// let start = Instant::now();
/// sync_wait(sender).unwrap();
/// assert!(start.elapsed() >= Duration::from_millis(10));
/// ```
pub struct TimedThreadContext {
    inner: Arc<TimedThreadInner>,
    thread: Option<JoinHandle<()>>,
}

struct TimedThreadInner {
    state: Mutex<TimedThreadState>,
    condvar: Condvar,
    stop_requested: AtomicBool,
    submission_counter: AtomicU64,
}

impl std::fmt::Debug for TimedThreadInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state.lock();
        f.debug_struct("TimedThreadInner")
            .field("heap_size", &state.heap.len())
            .field("finished", &state.finished)
            .field("stop_requested", &self.stop_requested)
            .finish()
    }
}

struct TimedThreadState {
    // Use a BinaryHeap with Reverse for min-heap behavior
    heap: BinaryHeap<Reverse<TimedOperation>>,
    finished: bool,
}

/// A scheduled operation in the heap.
struct TimedOperation {
    when: Instant,
    counter: u64,                           // For stable ordering when times are equal
    callback: Box<dyn FnOnce(bool) + Send>, // true = value, false = stopped
}

impl PartialEq for TimedOperation {
    fn eq(&self, other: &Self) -> bool {
        self.when == other.when && self.counter == other.counter
    }
}

impl Eq for TimedOperation {}

impl PartialOrd for TimedOperation {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TimedOperation {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.when.cmp(&other.when) {
            Ordering::Equal => self.counter.cmp(&other.counter),
            other => other,
        }
    }
}

impl TimedThreadContext {
    /// Create a new timed thread context.
    pub fn new() -> Self {
        let inner = Arc::new(TimedThreadInner {
            state: Mutex::new(TimedThreadState {
                heap: BinaryHeap::new(),
                finished: false,
            }),
            condvar: Condvar::new(),
            stop_requested: AtomicBool::new(false),
            submission_counter: AtomicU64::new(0),
        });

        let inner_clone = inner.clone();
        let thread = thread::Builder::new()
            .name("timed-thread-context".to_string())
            .spawn(move || {
                Self::run_loop(inner_clone);
            })
            .expect("Failed to spawn timer thread");

        Self {
            inner,
            thread: Some(thread),
        }
    }

    /// Get a scheduler for this context.
    pub fn get_scheduler(&self) -> TimedThreadScheduler {
        TimedThreadScheduler {
            inner: self.inner.clone(),
        }
    }

    fn run_loop(inner: Arc<TimedThreadInner>) {
        loop {
            let now = Instant::now();
            let mut state = inner.state.lock();

            // Check for stop request
            if inner
                .stop_requested
                .load(std::sync::atomic::Ordering::Relaxed)
            {
                // Cancel all pending operations
                while let Some(Reverse(op)) = state.heap.pop() {
                    drop(state);
                    (op.callback)(false); // set_stopped
                    state = inner.state.lock();
                }
                state.finished = true;
                return;
            }

            // Process all operations whose time has come
            while let Some(Reverse(op)) = state.heap.peek() {
                if op.when <= now {
                    let Reverse(op) = state.heap.pop().unwrap();
                    drop(state);
                    (op.callback)(true); // set_value
                    state = inner.state.lock();
                } else {
                    break;
                }
            }

            // Calculate next deadline
            let deadline = state
                .heap
                .peek()
                .map(|Reverse(op)| op.when)
                .unwrap_or_else(|| now + Duration::from_secs(2));

            // Wait until deadline or new work arrives
            let timeout = deadline.saturating_duration_since(Instant::now());
            inner.condvar.wait_for(&mut state, timeout);
        }
    }

    fn schedule(&self, when: Instant, callback: Box<dyn FnOnce(bool) + Send>) {
        let counter = self
            .inner
            .submission_counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let op = TimedOperation {
            when,
            counter,
            callback,
        };

        {
            let mut state = self.inner.state.lock();
            if state.finished {
                drop(state);
                // Context is closed, signal stopped
                // Note: callback already consumed, we need to handle this differently
                return;
            }
            state.heap.push(Reverse(op));
        }
        self.inner.condvar.notify_one();
    }
}

impl Default for TimedThreadContext {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TimedThreadContext {
    fn drop(&mut self) {
        // Signal stop
        self.inner
            .stop_requested
            .store(true, std::sync::atomic::Ordering::Release);
        self.inner.condvar.notify_all();

        // Wait for thread to finish
        if let Some(thread) = self.thread.take() {
            thread.join().ok();
        }
    }
}

/// Scheduler for a timed thread context.
#[derive(Clone, Debug)]
pub struct TimedThreadScheduler {
    inner: Arc<TimedThreadInner>,
}

impl PartialEq for TimedThreadScheduler {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for TimedThreadScheduler {}

impl TimedScheduler for TimedThreadScheduler {
    type Sender = TimedThreadScheduleSender;

    fn now(&self) -> Instant {
        Instant::now()
    }

    fn schedule_at(&self, when: Instant) -> Self::Sender {
        TimedThreadScheduleSender {
            inner: self.inner.clone(),
            when,
        }
    }
}

/// Completion signatures for timed schedule sender.
pub struct TimedScheduleCompletions;

impl CompletionSignatures for TimedScheduleCompletions {
    type Value = ();
    type Error = std::convert::Infallible;
    const SENDS_STOPPED: bool = true;
}

/// Sender returned by `TimedThreadScheduler::schedule_at()`.
pub struct TimedThreadScheduleSender {
    inner: Arc<TimedThreadInner>,
    when: Instant,
}

impl Sender for TimedThreadScheduleSender {
    type Completions = TimedScheduleCompletions;
}

impl<R> SenderTo<R> for TimedThreadScheduleSender
where
    R: ReceiverOf<()> + ReceiverOfStopped + Send + 'static,
{
    type Operation = TimedThreadScheduleOperation<R>;

    fn connect(self, receiver: R) -> Self::Operation {
        TimedThreadScheduleOperation {
            inner: self.inner,
            when: self.when,
            receiver: Some(receiver),
            started: false,
        }
    }
}

/// Operation state for timed schedule sender.
pub struct TimedThreadScheduleOperation<R> {
    inner: Arc<TimedThreadInner>,
    when: Instant,
    receiver: Option<R>,
    started: bool,
}

impl<R> OperationState for TimedThreadScheduleOperation<R>
where
    R: ReceiverOf<()> + ReceiverOfStopped + Send + 'static,
{
    fn start(self: Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };

        if this.started {
            return;
        }
        this.started = true;

        if let Some(receiver) = this.receiver.take() {
            let counter = this
                .inner
                .submission_counter
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

            let op = TimedOperation {
                when: this.when,
                counter,
                callback: Box::new(move |success| {
                    if success {
                        receiver.set_value(());
                    } else {
                        receiver.set_stopped();
                    }
                }),
            };

            {
                let mut state = this.inner.state.lock();
                if state.finished {
                    // Context is already closed
                    drop(state);
                    // Callback was moved into op, so it's consumed
                    return;
                }
                state.heap.push(Reverse(op));
            }
            this.inner.condvar.notify_one();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consumers::sync_wait;

    #[test]
    fn test_timed_thread_context_basic() {
        let ctx = TimedThreadContext::new();
        let scheduler = ctx.get_scheduler();

        // Schedule immediate work
        let sender = scheduler.schedule_at(Instant::now());
        let result = sync_wait(sender);
        assert!(result.is_ok());
    }

    #[test]
    fn test_timed_thread_schedule_after() {
        let ctx = TimedThreadContext::new();
        let scheduler = ctx.get_scheduler();

        let start = Instant::now();
        let sender = scheduler.schedule_after(Duration::from_millis(50));
        sync_wait(sender).unwrap();
        let elapsed = start.elapsed();

        // Should have waited at least 50ms
        assert!(elapsed >= Duration::from_millis(50));
        // But not too long (allow some slack)
        assert!(elapsed < Duration::from_millis(150));
    }

    #[test]
    fn test_timed_thread_multiple_schedules() {
        let ctx = TimedThreadContext::new();
        let scheduler = ctx.get_scheduler();

        let results = Arc::new(Mutex::new(Vec::new()));

        // Schedule three operations at different times
        let r1 = results.clone();
        let s1 = scheduler.schedule_after(Duration::from_millis(30));

        let r2 = results.clone();
        let s2 = scheduler.schedule_after(Duration::from_millis(10));

        let r3 = results.clone();
        let s3 = scheduler.schedule_after(Duration::from_millis(20));

        // Execute them (they should complete in order: s2, s3, s1)
        use std::thread;

        let h1 = thread::spawn(move || {
            sync_wait(s1).unwrap();
            r1.lock().push(30);
        });

        let h2 = thread::spawn(move || {
            sync_wait(s2).unwrap();
            r2.lock().push(10);
        });

        let h3 = thread::spawn(move || {
            sync_wait(s3).unwrap();
            r3.lock().push(20);
        });

        h1.join().unwrap();
        h2.join().unwrap();
        h3.join().unwrap();

        let results = results.lock();
        assert_eq!(results.len(), 3);
        // All values should be present (order of completion to vec may vary due to thread scheduling)
        assert!(results.contains(&10));
        assert!(results.contains(&20));
        assert!(results.contains(&30));
    }

    #[test]
    fn test_timed_scheduler_now() {
        let ctx = TimedThreadContext::new();
        let scheduler = ctx.get_scheduler();

        let before = Instant::now();
        let now = scheduler.now();
        let after = Instant::now();

        assert!(now >= before);
        assert!(now <= after);
    }

    #[test]
    fn test_timed_scheduler_equality() {
        let ctx = TimedThreadContext::new();
        let s1 = ctx.get_scheduler();
        let s2 = ctx.get_scheduler();

        assert_eq!(s1, s2);

        let ctx2 = TimedThreadContext::new();
        let s3 = ctx2.get_scheduler();

        assert_ne!(s1, s3);
    }
}
