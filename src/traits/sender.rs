//! Sender traits for lazy asynchronous work.
//!
//! A sender is a lazy description of asynchronous work. It doesn't execute
//! until connected to a receiver and started. This allows for composition
//! and optimization before execution begins.

use crate::traits::{CompletionSignatures, OperationState, Receiver};
use std::marker::PhantomData;

/// The core sender trait.
///
/// A sender describes lazy asynchronous work that will eventually produce
/// completion signals. The `Completions` associated type describes what
/// kinds of completions this sender can produce.
///
/// # Lazy Evaluation
///
/// Senders are lazy - they don't do any work until connected to a receiver
/// and started. This allows for:
/// - Composition without allocation
/// - Optimization across sender chains
/// - Cancellation before work begins
pub trait Sender: Sized {
    /// The completion signatures this sender can produce.
    type Completions: CompletionSignatures;
}

/// A sender that can be connected to a specific receiver type.
///
/// The `connect` method produces an operation state that, when started,
/// will execute the sender's work and deliver results to the receiver.
pub trait SenderTo<R: Receiver>: Sender {
    /// The operation state type produced by connecting this sender to the receiver.
    type Operation: OperationState;

    /// Connect this sender to a receiver.
    ///
    /// This creates an operation state that owns both the sender's work
    /// and the receiver. The operation won't start until `start()` is called.
    fn connect(self, receiver: R) -> Self::Operation;
}

/// A typed sender with explicit value and error types.
///
/// This is a convenience trait for senders with known value and error types.
pub trait TypedSender: Sender {
    /// The value type produced on successful completion.
    type Value;
    /// The error type produced on error completion.
    type Error;
}

/// Blanket implementation for senders with typed completions.
impl<S> TypedSender for S
where
    S: Sender,
    S::Completions: CompletionSignatures,
{
    type Value = <S::Completions as CompletionSignatures>::Value;
    type Error = <S::Completions as CompletionSignatures>::Error;
}

/// Extension trait providing fluent API methods on senders.
pub trait SenderExt: Sender {
    /// Apply a transformation function to the value.
    ///
    /// ```rust,ignore
    /// just(21).then(|x| x * 2)  // produces 42
    /// ```
    fn then<F, U>(self, f: F) -> crate::adaptors::Then<Self, F>
    where
        Self: Sized,
        F: FnOnce(<Self::Completions as CompletionSignatures>::Value) -> U,
    {
        crate::adaptors::Then::new(self, f)
    }

    /// Handle errors by applying a transformation function.
    fn upon_error<F, U>(self, f: F) -> crate::adaptors::UponError<Self, F>
    where
        Self: Sized,
        F: FnOnce(<Self::Completions as CompletionSignatures>::Error) -> U,
    {
        crate::adaptors::UponError::new(self, f)
    }

    /// Handle stopped signal by applying a function.
    fn upon_stopped<F, U>(self, f: F) -> crate::adaptors::UponStopped<Self, F>
    where
        Self: Sized,
        F: FnOnce() -> U,
    {
        crate::adaptors::UponStopped::new(self, f)
    }

    /// Chain with a function that returns a new sender.
    ///
    /// ```rust,ignore
    /// just(21).let_value(|x| just(x * 2))  // produces 42
    /// ```
    fn let_value<F, S2>(self, f: F) -> crate::adaptors::LetValue<Self, F>
    where
        Self: Sized,
        F: FnOnce(<Self::Completions as CompletionSignatures>::Value) -> S2,
        S2: Sender,
    {
        crate::adaptors::LetValue::new(self, f)
    }

    /// Handle errors with a function that returns a new sender.
    fn let_error<F, S2>(self, f: F) -> crate::adaptors::LetError<Self, F>
    where
        Self: Sized,
        F: FnOnce(<Self::Completions as CompletionSignatures>::Error) -> S2,
        S2: Sender,
    {
        crate::adaptors::LetError::new(self, f)
    }

    /// Handle stopped with a function that returns a new sender.
    fn let_stopped<F, S2>(self, f: F) -> crate::adaptors::LetStopped<Self, F>
    where
        Self: Sized,
        F: FnOnce() -> S2,
        S2: Sender,
    {
        crate::adaptors::LetStopped::new(self, f)
    }

    /// Start execution on a specific scheduler.
    fn starts_on<Sch>(self, scheduler: Sch) -> crate::adaptors::StartsOn<Self, Sch>
    where
        Self: Sized,
        Sch: crate::traits::Scheduler,
    {
        crate::adaptors::StartsOn::new(self, scheduler)
    }

    /// Continue execution on a specific scheduler.
    fn continues_on<Sch>(self, scheduler: Sch) -> crate::adaptors::ContinuesOn<Self, Sch>
    where
        Self: Sized,
        Sch: crate::traits::Scheduler,
    {
        crate::adaptors::ContinuesOn::new(self, scheduler)
    }

    /// Execute a function for each index in a range.
    ///
    /// When the upstream sender completes with a value, the function is called
    /// with each index from 0 to shape (exclusive) and a mutable reference to the value.
    ///
    /// ```rust,ignore
    /// just(0).bulk(10, |i, sum| { *sum += i; })
    /// ```
    fn bulk<F>(self, shape: usize, func: F) -> crate::adaptors::Bulk<Self, F>
    where
        Self: Sized,
        F: FnMut(usize, &mut <Self::Completions as CompletionSignatures>::Value),
    {
        crate::adaptors::Bulk::new(self, shape, func)
    }

    /// Convert a single-shot sender into a multi-shot sender.
    ///
    /// The underlying sender is started at most once, and its result
    /// is shared with all waiting receivers.
    fn split(self) -> crate::adaptors::Split<Self>
    where
        Self: Sized,
    {
        crate::adaptors::split(self)
    }

    /// Transform stopped completion to `None`, values to `Some`.
    fn stopped_as_optional(self) -> crate::adaptors::StoppedAsOptional<Self>
    where
        Self: Sized,
    {
        crate::adaptors::StoppedAsOptional::new(self)
    }

    /// Transform stopped completion to an error.
    fn stopped_as_error<E>(self, error: E) -> crate::adaptors::StoppedAsError<Self, E>
    where
        Self: Sized,
    {
        crate::adaptors::StoppedAsError::new(self, error)
    }

    /// Wrap value completion in `Option<T>`.
    fn into_variant(self) -> crate::adaptors::IntoVariant<Self>
    where
        Self: Sized,
    {
        crate::adaptors::IntoVariant::new(self)
    }

    /// Wrap value in a single-element tuple.
    fn into_tuple(self) -> crate::adaptors::IntoTuple<Self>
    where
        Self: Sized,
    {
        crate::adaptors::IntoTuple::new(self)
    }

    /// Materialize all completions into a `Completion` enum value.
    fn materialize(self) -> crate::adaptors::Materialize<Self>
    where
        Self: Sized,
    {
        crate::adaptors::Materialize::new(self)
    }

    /// Run cleanup sender after this sender completes (regardless of how).
    fn finally<S2>(self, cleanup: S2) -> crate::adaptors::Finally<Self, S2>
    where
        Self: Sized,
        S2: Sender,
    {
        crate::adaptors::Finally::new(self, cleanup)
    }

    /// Run this sender on a specific scheduler.
    fn on<Sch>(self, scheduler: Sch) -> crate::adaptors::On<Sch, Self>
    where
        Self: Sized,
        Sch: crate::traits::Scheduler,
    {
        crate::adaptors::On::new(scheduler, self)
    }

    /// Check stop token and complete with stopped if requested.
    fn unless_stopped(self) -> crate::adaptors::UnlessStopped<Self>
    where
        Self: Sized,
    {
        crate::adaptors::UnlessStopped::new(self)
    }
}

// Blanket implementation for all senders
impl<S: Sender> SenderExt for S {}

/// A sender that wraps a value and provides it to a function.
///
/// This is useful for building sender adaptors.
pub struct WrapSender<V, F> {
    value: V,
    factory: F,
}

impl<V, F> WrapSender<V, F> {
    /// Create a new wrapped sender.
    pub fn new(value: V, factory: F) -> Self {
        Self { value, factory }
    }
}

/// Marker type for dependent senders.
///
/// A dependent sender is one whose completion signatures depend on the
/// environment of the connected receiver.
#[derive(Debug, Clone, Copy)]
pub struct DependentSender<S>(PhantomData<S>);

/// Helper trait to check if a sender is connectable to a receiver.
pub trait Connectable<R: Receiver>: Sender {
    /// Check if this sender can be connected to the receiver type.
    fn is_connectable() -> bool {
        true
    }
}

// Blanket implementation
impl<S: SenderTo<R>, R: Receiver> Connectable<R> for S {}
