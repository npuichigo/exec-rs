//! Execution contexts and schedulers.
//!
//! Schedulers provide execution contexts for running senders:
//!
//! - [`InlineScheduler`]: Execute work immediately on the calling thread
//! - [`RunLoop`]: A single-threaded event loop
//! - [`SingleThreadContext`]: A dedicated background thread with a run loop
//! - [`StaticThreadPool`]: A multi-threaded work-stealing thread pool
//! - [`TrampolineScheduler`]: Recursion-safe inline scheduler
//! - [`ManualScheduler`]: Manually-driven scheduler for testing
//! - [`TimedThreadContext`]: A timed scheduler with a dedicated timer thread

mod inline;
mod manual;
mod run_loop;
mod single_thread;
mod thread_pool;
mod timed;
mod timed_thread;
mod trampoline;

pub use inline::*;
pub use manual::*;
pub use run_loop::*;
pub use single_thread::*;
pub use thread_pool::*;
pub use timed::*;
pub use timed_thread::*;
pub use trampoline::*;
