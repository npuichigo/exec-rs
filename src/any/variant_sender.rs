//! Variant sender - an enum of different sender types.
//!
//! `VariantSender` allows holding one of several sender types in a single type,
//! enabling runtime selection between different senders while maintaining
//! static typing for the completion signatures.

use crate::traits::{
    CompletionSignatures, OperationState, Receiver, ReceiverOf, ReceiverOfError,
    ReceiverOfStopped, Sender, SenderTo,
};
use std::pin::Pin;

// ============================================================================
// VariantSender2 - two sender variants
// ============================================================================

/// A sender that can be one of two types.
///
/// This allows runtime selection between two different sender types while
/// maintaining static typing. Both senders must have compatible completion
/// signatures.
///
/// # Example
///
/// ```rust
/// use exec_rs::any::VariantSender2;
/// use exec_rs::factories::just;
/// use exec_rs::consumers::sync_wait;
///
/// fn get_sender(use_first: bool) -> VariantSender2<
///     exec_rs::factories::Just<i32>,
///     exec_rs::factories::Just<i32>,
/// > {
///     if use_first {
///         VariantSender2::First(just(42))
///     } else {
///         VariantSender2::Second(just(100))
///     }
/// }
///
/// let result = sync_wait(get_sender(true)).unwrap();
/// assert_eq!(result, 42);
/// ```
#[derive(Debug, Clone)]
pub enum VariantSender2<S1, S2> {
    First(S1),
    Second(S2),
}

impl<S1, S2> VariantSender2<S1, S2> {
    /// Create a variant sender from the first type.
    pub fn first(sender: S1) -> Self {
        VariantSender2::First(sender)
    }

    /// Create a variant sender from the second type.
    pub fn second(sender: S2) -> Self {
        VariantSender2::Second(sender)
    }
}

impl<S1, S2, V, E> Sender for VariantSender2<S1, S2>
where
    S1: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
    S2: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
{
    type Completions = VariantCompletions2<S1, S2>;
}

/// Completion signatures for VariantSender2.
pub struct VariantCompletions2<S1: Sender, S2: Sender> {
    _phantom: std::marker::PhantomData<(S1, S2)>,
}

impl<S1, S2, V, E> CompletionSignatures for VariantCompletions2<S1, S2>
where
    S1: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
    S2: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
{
    type Value = V;
    type Error = E;
    const SENDS_STOPPED: bool =
        S1::Completions::SENDS_STOPPED || S2::Completions::SENDS_STOPPED;
}

impl<S1, S2, V, E, R> SenderTo<R> for VariantSender2<S1, S2>
where
    S1: Sender<Completions: CompletionSignatures<Value = V, Error = E>> + SenderTo<R>,
    S2: Sender<Completions: CompletionSignatures<Value = V, Error = E>> + SenderTo<R>,
    R: ReceiverOf<V> + ReceiverOfError<E> + ReceiverOfStopped,
{
    type Operation = VariantOperation2<S1::Operation, S2::Operation>;

    fn connect(self, receiver: R) -> Self::Operation {
        match self {
            VariantSender2::First(s) => VariantOperation2::First(s.connect(receiver)),
            VariantSender2::Second(s) => VariantOperation2::Second(s.connect(receiver)),
        }
    }
}

/// Operation state for VariantSender2.
pub enum VariantOperation2<Op1, Op2> {
    First(Op1),
    Second(Op2),
}

impl<Op1: OperationState, Op2: OperationState> OperationState for VariantOperation2<Op1, Op2> {
    fn start(self: Pin<&mut Self>) {
        unsafe {
            match self.get_unchecked_mut() {
                VariantOperation2::First(op) => Pin::new_unchecked(op).start(),
                VariantOperation2::Second(op) => Pin::new_unchecked(op).start(),
            }
        }
    }
}

// ============================================================================
// VariantSender3 - three sender variants
// ============================================================================

/// A sender that can be one of three types.
#[derive(Debug, Clone)]
pub enum VariantSender3<S1, S2, S3> {
    First(S1),
    Second(S2),
    Third(S3),
}

impl<S1, S2, S3> VariantSender3<S1, S2, S3> {
    pub fn first(sender: S1) -> Self {
        VariantSender3::First(sender)
    }

    pub fn second(sender: S2) -> Self {
        VariantSender3::Second(sender)
    }

    pub fn third(sender: S3) -> Self {
        VariantSender3::Third(sender)
    }
}

impl<S1, S2, S3, V, E> Sender for VariantSender3<S1, S2, S3>
where
    S1: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
    S2: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
    S3: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
{
    type Completions = VariantCompletions3<S1, S2, S3>;
}

/// Completion signatures for VariantSender3.
pub struct VariantCompletions3<S1: Sender, S2: Sender, S3: Sender> {
    _phantom: std::marker::PhantomData<(S1, S2, S3)>,
}

impl<S1, S2, S3, V, E> CompletionSignatures for VariantCompletions3<S1, S2, S3>
where
    S1: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
    S2: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
    S3: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
{
    type Value = V;
    type Error = E;
    const SENDS_STOPPED: bool = S1::Completions::SENDS_STOPPED
        || S2::Completions::SENDS_STOPPED
        || S3::Completions::SENDS_STOPPED;
}

impl<S1, S2, S3, V, E, R> SenderTo<R> for VariantSender3<S1, S2, S3>
where
    S1: Sender<Completions: CompletionSignatures<Value = V, Error = E>> + SenderTo<R>,
    S2: Sender<Completions: CompletionSignatures<Value = V, Error = E>> + SenderTo<R>,
    S3: Sender<Completions: CompletionSignatures<Value = V, Error = E>> + SenderTo<R>,
    R: ReceiverOf<V> + ReceiverOfError<E> + ReceiverOfStopped,
{
    type Operation = VariantOperation3<S1::Operation, S2::Operation, S3::Operation>;

    fn connect(self, receiver: R) -> Self::Operation {
        match self {
            VariantSender3::First(s) => VariantOperation3::First(s.connect(receiver)),
            VariantSender3::Second(s) => VariantOperation3::Second(s.connect(receiver)),
            VariantSender3::Third(s) => VariantOperation3::Third(s.connect(receiver)),
        }
    }
}

/// Operation state for VariantSender3.
pub enum VariantOperation3<Op1, Op2, Op3> {
    First(Op1),
    Second(Op2),
    Third(Op3),
}

impl<Op1: OperationState, Op2: OperationState, Op3: OperationState> OperationState
    for VariantOperation3<Op1, Op2, Op3>
{
    fn start(self: Pin<&mut Self>) {
        unsafe {
            match self.get_unchecked_mut() {
                VariantOperation3::First(op) => Pin::new_unchecked(op).start(),
                VariantOperation3::Second(op) => Pin::new_unchecked(op).start(),
                VariantOperation3::Third(op) => Pin::new_unchecked(op).start(),
            }
        }
    }
}

// ============================================================================
// VariantSender4 - four sender variants
// ============================================================================

/// A sender that can be one of four types.
#[derive(Debug, Clone)]
pub enum VariantSender4<S1, S2, S3, S4> {
    First(S1),
    Second(S2),
    Third(S3),
    Fourth(S4),
}

impl<S1, S2, S3, S4> VariantSender4<S1, S2, S3, S4> {
    pub fn first(sender: S1) -> Self {
        VariantSender4::First(sender)
    }

    pub fn second(sender: S2) -> Self {
        VariantSender4::Second(sender)
    }

    pub fn third(sender: S3) -> Self {
        VariantSender4::Third(sender)
    }

    pub fn fourth(sender: S4) -> Self {
        VariantSender4::Fourth(sender)
    }
}

impl<S1, S2, S3, S4, V, E> Sender for VariantSender4<S1, S2, S3, S4>
where
    S1: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
    S2: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
    S3: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
    S4: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
{
    type Completions = VariantCompletions4<S1, S2, S3, S4>;
}

/// Completion signatures for VariantSender4.
pub struct VariantCompletions4<S1: Sender, S2: Sender, S3: Sender, S4: Sender> {
    _phantom: std::marker::PhantomData<(S1, S2, S3, S4)>,
}

impl<S1, S2, S3, S4, V, E> CompletionSignatures for VariantCompletions4<S1, S2, S3, S4>
where
    S1: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
    S2: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
    S3: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
    S4: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
{
    type Value = V;
    type Error = E;
    const SENDS_STOPPED: bool = S1::Completions::SENDS_STOPPED
        || S2::Completions::SENDS_STOPPED
        || S3::Completions::SENDS_STOPPED
        || S4::Completions::SENDS_STOPPED;
}

impl<S1, S2, S3, S4, V, E, R> SenderTo<R> for VariantSender4<S1, S2, S3, S4>
where
    S1: Sender<Completions: CompletionSignatures<Value = V, Error = E>> + SenderTo<R>,
    S2: Sender<Completions: CompletionSignatures<Value = V, Error = E>> + SenderTo<R>,
    S3: Sender<Completions: CompletionSignatures<Value = V, Error = E>> + SenderTo<R>,
    S4: Sender<Completions: CompletionSignatures<Value = V, Error = E>> + SenderTo<R>,
    R: ReceiverOf<V> + ReceiverOfError<E> + ReceiverOfStopped,
{
    type Operation =
        VariantOperation4<S1::Operation, S2::Operation, S3::Operation, S4::Operation>;

    fn connect(self, receiver: R) -> Self::Operation {
        match self {
            VariantSender4::First(s) => VariantOperation4::First(s.connect(receiver)),
            VariantSender4::Second(s) => VariantOperation4::Second(s.connect(receiver)),
            VariantSender4::Third(s) => VariantOperation4::Third(s.connect(receiver)),
            VariantSender4::Fourth(s) => VariantOperation4::Fourth(s.connect(receiver)),
        }
    }
}

/// Operation state for VariantSender4.
pub enum VariantOperation4<Op1, Op2, Op3, Op4> {
    First(Op1),
    Second(Op2),
    Third(Op3),
    Fourth(Op4),
}

impl<Op1: OperationState, Op2: OperationState, Op3: OperationState, Op4: OperationState>
    OperationState for VariantOperation4<Op1, Op2, Op3, Op4>
{
    fn start(self: Pin<&mut Self>) {
        unsafe {
            match self.get_unchecked_mut() {
                VariantOperation4::First(op) => Pin::new_unchecked(op).start(),
                VariantOperation4::Second(op) => Pin::new_unchecked(op).start(),
                VariantOperation4::Third(op) => Pin::new_unchecked(op).start(),
                VariantOperation4::Fourth(op) => Pin::new_unchecked(op).start(),
            }
        }
    }
}

// ============================================================================
// Convenience type aliases
// ============================================================================

/// Type alias for VariantSender2.
pub type EitherSender<S1, S2> = VariantSender2<S1, S2>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consumers::sync_wait;
    use crate::factories::{just, just_error, Just, JustError};

    #[test]
    fn test_variant_sender2_first() {
        let sender: VariantSender2<Just<i32>, Just<i32>> = VariantSender2::first(just(42));
        let result = sync_wait(sender).unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn test_variant_sender2_second() {
        let sender: VariantSender2<Just<i32>, Just<i32>> = VariantSender2::second(just(100));
        let result = sync_wait(sender).unwrap();
        assert_eq!(result, 100);
    }

    #[test]
    fn test_variant_sender2_runtime_selection() {
        fn get_sender(use_first: bool) -> VariantSender2<Just<i32>, Just<i32>> {
            if use_first {
                VariantSender2::First(just(42))
            } else {
                VariantSender2::Second(just(100))
            }
        }

        assert_eq!(sync_wait(get_sender(true)).unwrap(), 42);
        assert_eq!(sync_wait(get_sender(false)).unwrap(), 100);
    }

    #[test]
    fn test_variant_sender3() {
        let sender: VariantSender3<Just<&str>, Just<&str>, Just<&str>> =
            VariantSender3::third(just("hello"));
        let result = sync_wait(sender).unwrap();
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_variant_sender4() {
        let sender: VariantSender4<Just<f64>, Just<f64>, Just<f64>, Just<f64>> =
            VariantSender4::fourth(just(3.14f64));
        let result = sync_wait(sender).unwrap();
        assert!((result - 3.14).abs() < 0.001);
    }

    #[test]
    fn test_variant_sender_error() {
        let sender: VariantSender2<JustError<&str>, JustError<&str>> =
            VariantSender2::first(just_error("error"));
        let result = sync_wait(sender);
        assert!(result.is_err());
    }

    #[test]
    fn test_either_sender_alias() {
        let sender: EitherSender<Just<i32>, Just<i32>> = EitherSender::first(just(42));
        let result = sync_wait(sender).unwrap();
        assert_eq!(result, 42);
    }
}
