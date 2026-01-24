//! Utilities.
//!
//! Supporting infrastructure:
//! - `ScopeGuard`: RAII cleanup
//! - `AtomicIntrusiveQueue`: Lock-free queue
//! - `IntrusiveHeap`: Priority heap for timers

mod intrusive_heap;
mod scope_guard;

pub use intrusive_heap::*;
pub use scope_guard::*;
