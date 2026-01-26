//! into_variant - Wraps a sender's value completion into a variant type.
//!
//! This adaptor wraps the value completion of a sender into an `Option<T>`
//! (for single-value senders) or a tuple variant (for multi-value scenarios).
//! This is useful when you need a uniform type across different sender paths.

use crate::traits::{
    CompletionSignatures, OperationState, Receiver, ReceiverOf, ReceiverOfError, ReceiverOfStopped,
    Sender, SenderTo,
};
use std::marker::PhantomData;
use std::pin::Pin;

/// Wraps a sender's value completion into an `Option<T>`.
///
/// The resulting sender completes with:
/// - `Some(value)` when the inner sender completes with a value
/// - The inner sender's error completions are preserved
/// - The inner sender's stopped completion is preserved
///
/// # Example
///
/// ```rust
/// use exec_rs::factories::just;
/// use exec_rs::adaptors::into_variant;
/// use exec_rs::consumers::sync_wait;
///
/// let result = sync_wait(into_variant(just(42))).unwrap();
/// assert_eq!(result, Some(42));
/// ```
pub fn into_variant<S>(sender: S) -> IntoVariant<S> {
    IntoVariant { sender }
}

/// A sender that wraps its value completion in Option.
#[derive(Debug, Clone)]
pub struct IntoVariant<S> {
    sender: S,
}

/// Completion signatures for IntoVariant.
pub struct IntoVariantCompletions<V, E>(PhantomData<(V, E)>);

impl<V, E> CompletionSignatures for IntoVariantCompletions<V, E> {
    type Value = Option<V>;
    type Error = E;
    const SENDS_STOPPED: bool = true;
}

impl<S, V, E> Sender for IntoVariant<S>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
{
    type Completions = IntoVariantCompletions<V, E>;
}

impl<S, R, V, E> SenderTo<R> for IntoVariant<S>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>
        + SenderTo<IntoVariantReceiver<R, V, E>>,
    R: ReceiverOf<Option<V>> + ReceiverOfError<E> + ReceiverOfStopped,
{
    type Operation = IntoVariantOperation<S, R, V, E>;

    fn connect(self, receiver: R) -> Self::Operation {
        let inner_receiver = IntoVariantReceiver {
            receiver: Some(receiver),
            _phantom: PhantomData,
        };
        IntoVariantOperation {
            inner: self.sender.connect(inner_receiver),
        }
    }
}

/// Receiver that wraps values in Option.
pub struct IntoVariantReceiver<R, V, E> {
    receiver: Option<R>,
    _phantom: PhantomData<(V, E)>,
}

impl<R: Receiver, V, E> Receiver for IntoVariantReceiver<R, V, E> {
    type Env = R::Env;

    fn get_env(&self) -> &Self::Env {
        self.receiver.as_ref().unwrap().get_env()
    }
}

impl<R, V, E> ReceiverOf<V> for IntoVariantReceiver<R, V, E>
where
    R: ReceiverOf<Option<V>>,
{
    fn set_value(mut self, value: V) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_value(Some(value));
        }
    }
}

impl<R, V, E> ReceiverOfError<E> for IntoVariantReceiver<R, V, E>
where
    R: ReceiverOfError<E>,
{
    fn set_error(mut self, error: E) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_error(error);
        }
    }
}

impl<R, V, E> ReceiverOfStopped for IntoVariantReceiver<R, V, E>
where
    R: ReceiverOfStopped,
{
    fn set_stopped(mut self) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_stopped();
        }
    }
}

/// Operation state for IntoVariant.
pub struct IntoVariantOperation<S, R, V, E>
where
    R: Receiver,
    S: SenderTo<IntoVariantReceiver<R, V, E>>,
{
    inner: S::Operation,
}

impl<S, R, V, E> OperationState for IntoVariantOperation<S, R, V, E>
where
    R: Receiver,
    S: SenderTo<IntoVariantReceiver<R, V, E>>,
{
    fn start(self: Pin<&mut Self>) {
        unsafe {
            let this = self.get_unchecked_mut();
            Pin::new_unchecked(&mut this.inner).start();
        }
    }
}

/// Extension trait for into_variant.
pub trait IntoVariantExt: Sender + Sized {
    /// Wrap this sender's value completion in Option.
    fn into_variant(self) -> IntoVariant<Self> {
        into_variant(self)
    }
}

impl<S: Sender> IntoVariantExt for S {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consumers::sync_wait;
    use crate::factories::{just, just_error};

    #[test]
    fn test_into_variant_value() {
        let result = sync_wait(into_variant(just(42))).unwrap();
        assert_eq!(result, Some(42));
    }

    #[test]
    fn test_into_variant_string() {
        let result = sync_wait(into_variant(just("hello"))).unwrap();
        assert_eq!(result, Some("hello"));
    }

    #[test]
    fn test_into_variant_preserves_error() {
        let result: Result<Option<()>, _> = sync_wait(into_variant(just_error("oops")));
        assert!(result.is_err());
    }

    #[test]
    fn test_into_variant_extension() {
        let result = sync_wait(just(42).into_variant()).unwrap();
        assert_eq!(result, Some(42));
    }

    #[test]
    fn test_into_variant_unit() {
        let result = sync_wait(into_variant(just(()))).unwrap();
        assert_eq!(result, Some(()));
    }

    #[test]
    fn test_into_variant_tuple() {
        let result = sync_wait(into_variant(just((1, 2, 3)))).unwrap();
        assert_eq!(result, Some((1, 2, 3)));
    }
}
