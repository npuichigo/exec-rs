//! Type-erased senders and schedulers.
//!
//! This module provides runtime polymorphism for senders:
//! - [`AnySender`]: Fully type-erased sender (heap allocated)
//! - [`AnyScheduler`]: Type-erased scheduler
//! - [`VariantSender2`], [`VariantSender3`], [`VariantSender4`]: Enum-based variant senders
//! - [`EitherSender`]: Alias for `VariantSender2`
//!
//! ## AnySender vs VariantSender
//!
//! - Use `AnySender` when you need full type erasure and don't know the sender
//!   types at compile time. This uses heap allocation.
//! - Use `VariantSender` when you have a fixed set of possible sender types
//!   known at compile time. This is stack-allocated and more efficient.
//!
//! # Example
//!
//! ```rust
//! use exec_rs::any::{AnySender, VariantSender2};
//! use exec_rs::factories::just;
//! use exec_rs::consumers::sync_wait;
//!
//! // Using VariantSender (no heap allocation)
//! fn get_variant_sender(use_first: bool) -> VariantSender2<
//!     exec_rs::factories::Just<i32>,
//!     exec_rs::factories::Just<i32>,
//! > {
//!     if use_first {
//!         VariantSender2::First(just(42))
//!     } else {
//!         VariantSender2::Second(just(100))
//!     }
//! }
//!
//! // Using AnySender (heap allocated, fully erased)
//! fn get_any_sender(use_first: bool) -> AnySender<i32, std::convert::Infallible> {
//!     if use_first {
//!         AnySender::new(just(42))
//!     } else {
//!         AnySender::new(just(100))
//!     }
//! }
//!
//! assert_eq!(sync_wait(get_variant_sender(true)).unwrap(), 42);
//! assert_eq!(sync_wait(get_any_sender(false)).unwrap(), 100);
//! ```

mod any_scheduler;
mod any_sender;
mod variant_sender;

pub use any_scheduler::*;
pub use any_sender::*;
pub use variant_sender::*;
