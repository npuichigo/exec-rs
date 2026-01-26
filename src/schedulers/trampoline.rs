//! Trampoline scheduler - a recursion-safe inline scheduler.
//!
//! The trampoline scheduler prevents stack overflow by limiting recursion depth.
//! When the limit is exceeded, work is queued and drained iteratively.

use crate::traits::{
    CompletionSignatures, OperationState, ReceiverOf, ReceiverOfStopped, Scheduler, Sender,
    SenderTo,
};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::pin::Pin;

/// Default maximum recursion depth before queueing.
const DEFAULT_MAX_DEPTH: usize = 16;

/// Thread-local trampoline state.
struct TrampolineState {
    max_depth: usize,
    current_depth: usize,
    queue: VecDeque<Box<dyn FnOnce()>>,
}

thread_local! {
    static TRAMPOLINE: RefCell<Option<TrampolineState>> = const { RefCell::new(None) };
}

/// A recursion-safe inline scheduler.
///
/// The trampoline scheduler executes work inline up to a configurable recursion
/// depth. When the depth limit is exceeded, work is queued and drained iteratively
/// to prevent stack overflow.
///
/// # Example
///
/// ```rust
/// use exec_rs::schedulers::TrampolineScheduler;
/// use exec_rs::traits::Scheduler;
/// use exec_rs::consumers::sync_wait;
///
/// let scheduler = TrampolineScheduler::new();
/// let sender = scheduler.schedule();
/// let result = sync_wait(sender).unwrap();
/// assert_eq!(result, ());
/// ```
#[derive(Clone, Debug)]
pub struct TrampolineScheduler {
    max_depth: usize,
}

impl TrampolineScheduler {
    /// Create a new trampoline scheduler with default recursion depth limit.
    pub fn new() -> Self {
        Self {
            max_depth: DEFAULT_MAX_DEPTH,
        }
    }

    /// Create a new trampoline scheduler with a custom recursion depth limit.
    pub fn with_max_depth(max_depth: usize) -> Self {
        Self { max_depth }
    }
}

impl Default for TrampolineScheduler {
    fn default() -> Self {
        Self::new()
    }
}

/// Submit work to the trampoline, preventing stack overflow from recursive calls.
///
/// This function is useful for implementing sender adaptors that need to
/// start the next iteration from within a completion handler.
///
/// If called from within a trampoline context and the depth limit is exceeded,
/// the work is queued and executed later. Otherwise, the work is executed inline.
///
/// # Example
///
/// ```rust
/// use exec_rs::schedulers::trampoline_submit;
///
/// fn recursive_work(remaining: usize) {
///     if remaining > 0 {
///         trampoline_submit(move || {
///             recursive_work(remaining - 1);
///         });
///     }
/// }
///
/// // This won't overflow the stack even with large values
/// recursive_work(10000);
/// ```
pub fn trampoline_submit<F: FnOnce() + 'static>(work: F) {
    TRAMPOLINE.with(|cell| {
        let mut borrow = cell.borrow_mut();

        if borrow.is_none() {
            // No trampoline context - create one and execute
            *borrow = Some(TrampolineState {
                max_depth: DEFAULT_MAX_DEPTH,
                current_depth: 1,
                queue: VecDeque::new(),
            });
            drop(borrow);

            // Execute the work
            work();

            // Drain any queued work
            loop {
                let queued_work = TRAMPOLINE.with(|cell| {
                    let mut borrow = cell.borrow_mut();
                    if let Some(state) = borrow.as_mut() {
                        state.queue.pop_front()
                    } else {
                        None
                    }
                });

                match queued_work {
                    Some(queued_work) => queued_work(),
                    None => break,
                }
            }

            // Clean up state
            TRAMPOLINE.with(|cell| {
                *cell.borrow_mut() = None;
            });
        } else {
            // Already in trampoline context - check depth
            let state = borrow.as_mut().unwrap();
            if state.current_depth < state.max_depth {
                // Execute inline
                state.current_depth += 1;
                drop(borrow);
                work();
                TRAMPOLINE.with(|cell| {
                    if let Some(state) = cell.borrow_mut().as_mut() {
                        state.current_depth -= 1;
                    }
                });
            } else {
                // Queue for later execution
                state.queue.push_back(Box::new(work));
            }
        }
    });
}

impl PartialEq for TrampolineScheduler {
    fn eq(&self, other: &Self) -> bool {
        self.max_depth == other.max_depth
    }
}

impl Eq for TrampolineScheduler {}

impl Scheduler for TrampolineScheduler {
    type Sender = TrampolineScheduleSender;

    fn schedule(&self) -> Self::Sender {
        TrampolineScheduleSender {
            max_depth: self.max_depth,
        }
    }
}

/// Completion signatures for trampoline schedule sender.
pub struct TrampolineScheduleCompletions;

impl CompletionSignatures for TrampolineScheduleCompletions {
    type Value = ();
    type Error = std::convert::Infallible;
    const SENDS_STOPPED: bool = true;
}

/// Sender returned by `TrampolineScheduler::schedule()`.
#[derive(Clone, Debug)]
pub struct TrampolineScheduleSender {
    max_depth: usize,
}

impl Sender for TrampolineScheduleSender {
    type Completions = TrampolineScheduleCompletions;
}

impl<R> SenderTo<R> for TrampolineScheduleSender
where
    R: ReceiverOf<()> + ReceiverOfStopped + 'static,
{
    type Operation = TrampolineScheduleOperation<R>;

    fn connect(self, receiver: R) -> Self::Operation {
        TrampolineScheduleOperation {
            max_depth: self.max_depth,
            receiver: Some(receiver),
        }
    }
}

/// Operation state for trampoline schedule sender.
pub struct TrampolineScheduleOperation<R> {
    max_depth: usize,
    receiver: Option<R>,
}

impl<R: ReceiverOf<()> + ReceiverOfStopped + 'static> TrampolineScheduleOperation<R> {
    fn execute(receiver: R) {
        // Check stop token
        let env = receiver.get_env();
        let stop_token = crate::traits::Environment::get_stop_token(env);
        if crate::stop::StopToken::stop_requested(&stop_token) {
            receiver.set_stopped();
        } else {
            receiver.set_value(());
        }
    }
}

impl<R: ReceiverOf<()> + ReceiverOfStopped + 'static> OperationState
    for TrampolineScheduleOperation<R>
{
    fn start(self: Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };
        let Some(receiver) = this.receiver.take() else {
            return;
        };

        let max_depth = this.max_depth;

        TRAMPOLINE.with(|cell| {
            let mut borrow = cell.borrow_mut();

            if borrow.is_none() {
                // Origin frame - create new state and execute
                *borrow = Some(TrampolineState {
                    max_depth,
                    current_depth: 1,
                    queue: VecDeque::new(),
                });
                drop(borrow);

                // Execute the work
                Self::execute(receiver);

                // Drain any queued work
                loop {
                    let work = TRAMPOLINE.with(|cell| {
                        let mut borrow = cell.borrow_mut();
                        if let Some(state) = borrow.as_mut() {
                            state.queue.pop_front()
                        } else {
                            None
                        }
                    });

                    match work {
                        Some(work) => work(),
                        None => break,
                    }
                }

                // Clean up state
                TRAMPOLINE.with(|cell| {
                    *cell.borrow_mut() = None;
                });
            } else {
                // Recursive frame - check depth
                let state = borrow.as_mut().unwrap();
                if state.current_depth < state.max_depth {
                    // Execute inline
                    state.current_depth += 1;
                    drop(borrow);
                    Self::execute(receiver);
                    TRAMPOLINE.with(|cell| {
                        if let Some(state) = cell.borrow_mut().as_mut() {
                            state.current_depth -= 1;
                        }
                    });
                } else {
                    // Queue for later
                    state.queue.push_back(Box::new(move || {
                        Self::execute(receiver);
                    }));
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consumers::sync_wait;
    use crate::factories::just;
    use crate::traits::Receiver;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn test_trampoline_basic() {
        let scheduler = TrampolineScheduler::new();
        let sender = scheduler.schedule();
        let result = sync_wait(sender).unwrap();
        assert_eq!(result, ());
    }

    #[test]
    fn test_trampoline_with_value() {
        use crate::adaptors::on;

        let scheduler = TrampolineScheduler::new();
        let sender = on(scheduler, just(42));
        let result = sync_wait(sender).unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn test_trampoline_deep_recursion() {
        use crate::consumers::DetachedEnv;
        use std::pin::pin;

        // Test that deep recursion doesn't overflow the stack
        let scheduler = TrampolineScheduler::with_max_depth(4);
        let counter = Arc::new(AtomicUsize::new(0));

        // Create a receiver that schedules more work recursively
        struct RecursiveReceiver {
            scheduler: TrampolineScheduler,
            counter: Arc<AtomicUsize>,
            remaining: usize,
        }

        impl Receiver for RecursiveReceiver {
            type Env = DetachedEnv;
            fn get_env(&self) -> &Self::Env {
                &DetachedEnv
            }
        }

        impl ReceiverOf<()> for RecursiveReceiver {
            fn set_value(self, _: ()) {
                self.counter.fetch_add(1, Ordering::SeqCst);
                if self.remaining > 1 {
                    // Schedule another iteration
                    let sender = self.scheduler.schedule();
                    let receiver = RecursiveReceiver {
                        scheduler: self.scheduler.clone(),
                        counter: self.counter.clone(),
                        remaining: self.remaining - 1,
                    };
                    let mut op = pin!(sender.connect(receiver));
                    op.as_mut().start();
                }
            }
        }

        impl ReceiverOfStopped for RecursiveReceiver {
            fn set_stopped(self) {}
        }

        // Start the recursive chain
        let sender = scheduler.schedule();
        let receiver = RecursiveReceiver {
            scheduler: scheduler.clone(),
            counter: counter.clone(),
            remaining: 100,
        };
        let mut op = pin!(sender.connect(receiver));
        op.as_mut().start();

        // All iterations should complete
        assert_eq!(counter.load(Ordering::SeqCst), 100);
    }

    #[test]
    fn test_trampoline_scheduler_equality() {
        let s1 = TrampolineScheduler::new();
        let s2 = TrampolineScheduler::new();
        let s3 = TrampolineScheduler::with_max_depth(8);

        assert_eq!(s1, s2);
        assert_ne!(s1, s3);
    }
}
