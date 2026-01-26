//! Structured concurrency with async scopes.
//!
//! Scopes manage the lifetime of spawned senders, ensuring all work completes
//! before the scope is destroyed.
//!
//! ## Types
//!
//! - [`CountingScope`]: Simple reference-counting scope
//! - [`AsyncScope`]: Full async scope with nest, spawn, when_empty
//!
//! ## Example
//!
//! ```rust,ignore
//! use exec_rs::scope::CountingScope;
//! use exec_rs::factories::just;
//! use exec_rs::consumers::sync_wait;
//!
//! let scope = CountingScope::new();
//!
//! // Spawn some work
//! scope.spawn(just(1));
//! scope.spawn(just(2));
//!
//! // Wait for all work to complete
//! sync_wait(scope.when_empty()).unwrap();
//! ```

mod async_scope;
mod counting_scope;
mod nest;

pub use async_scope::*;
pub use counting_scope::*;
pub use nest::*;
