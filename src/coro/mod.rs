//! Coroutine integration for async/await interoperability.
//!
//! This module provides integration between exec-rs senders and Rust's
//! async/await system:
//!
//! - [`Task`]: A spawnable task with cancellation support
//! - [`Awaitable`]: Trait for making senders awaitable
//! - [`at_coroutine_exit`]: Cleanup guarantee for async code
//!
//! # Task Spawning
//!
//! ```rust,ignore
//! use exec_rs::coro::{Task, SpawnExt};
//! use exec_rs::factories::just;
//!
//! async fn example() {
//!     // Spawn a sender as a task
//!     let task = Task::spawn(just(42));
//!
//!     // Or use the extension trait
//!     let task = just(42).spawn_task();
//!
//!     // Wait for completion
//!     let result = task.await;
//!     assert_eq!(result, Ok(42));
//! }
//! ```
//!
//! # Making Senders Awaitable
//!
//! ```rust,ignore
//! use exec_rs::coro::Awaitable;
//! use exec_rs::factories::just;
//!
//! async fn example() {
//!     let value = just(42).into_awaitable().await;
//!     assert_eq!(value, Ok(42));
//! }
//! ```
//!
//! # Cleanup Guarantees
//!
//! ```rust
//! use exec_rs::coro::{at_coroutine_exit, scope_exit};
//!
//! fn sync_example() {
//!     let _guard = scope_exit(|| println!("Cleanup!"));
//!     // ... work ...
//! } // Cleanup runs here
//! ```
//!
//! # Stop Token Propagation
//!
//! Tasks support cancellation via stop tokens:
//!
//! ```rust,ignore
//! use exec_rs::coro::Task;
//! use exec_rs::factories::just;
//!
//! let task = Task::spawn(just(42));
//! task.cancel(); // Request cancellation
//! ```

mod at_exit;
mod awaitable;
mod task;

pub use at_exit::{at_coroutine_exit, scope_exit, AtCoroutineExit, DeferredCleanup, ScopeExit};
pub use awaitable::{Awaitable, ReadyAwaitable};
pub use task::{SpawnExt, Task, TaskError, TaskReceiver};
