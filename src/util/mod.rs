//! Utilities.
//!
//! Supporting infrastructure:
//! - `ScopeGuard`: RAII cleanup
//! - `AtomicIntrusiveQueue`: Lock-free MPSC queue
//! - `IntrusiveQueue`: Single-threaded intrusive queue
//! - `IntrusiveHeap`: Priority heap for timers

mod intrusive_heap;
mod intrusive_queue;
mod scope_guard;

pub use intrusive_heap::*;
pub use intrusive_queue::*;
pub use scope_guard::*;
