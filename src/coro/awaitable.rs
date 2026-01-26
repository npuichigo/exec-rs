//! Awaitable trait for making senders usable in async contexts.
//!
//! This module provides the `Awaitable` trait which allows any sender
//! to be converted to a Future that can be awaited.

use crate::future::{FutureReceiver, SenderFuture, SenderFutureError};
use crate::traits::{CompletionSignatures, Sender, SenderTo};
use std::future::Future;

/// Trait for types that can be awaited in async contexts.
///
/// This is automatically implemented for all `Sender` types that can
/// be converted to futures.
///
/// # Example
///
/// ```rust,ignore
/// use exec_rs::coro::Awaitable;
/// use exec_rs::factories::just;
///
/// async fn example() {
///     let value = just(42).into_awaitable().await;
///     assert_eq!(value, Ok(42));
/// }
/// ```
pub trait Awaitable: Sender {
    /// The output type when awaited.
    type Output;

    /// The future type produced by this awaitable.
    type AwaitFuture: Future<Output = Self::Output>;

    /// Convert this sender into a future that can be awaited.
    fn into_awaitable(self) -> Self::AwaitFuture;
}

impl<S, V, E> Awaitable for S
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
    S: SenderTo<FutureReceiver<V, E>>,
    V: Send + 'static,
    E: Send + 'static,
    S::Operation: Send + 'static,
{
    type Output = Result<V, SenderFutureError<E>>;
    type AwaitFuture = SenderFuture<S>;

    fn into_awaitable(self) -> Self::AwaitFuture {
        SenderFuture::new(self)
    }
}

/// Marker trait for types that are "ready" awaitables (immediately complete).
///
/// This is useful for optimizations where we know a sender will
/// complete synchronously.
pub trait ReadyAwaitable: Awaitable {}

// just() senders are ready awaitables
impl<T: Send + 'static> ReadyAwaitable for crate::factories::Just<T> {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::factories::just;
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    fn block_on<F: Future>(f: F) -> F::Output {
        static VTABLE: RawWakerVTable = RawWakerVTable::new(
            |_| RawWaker::new(std::ptr::null(), &VTABLE),
            |_| {},
            |_| {},
            |_| {},
        );

        let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
        let mut cx = Context::from_waker(&waker);
        let mut f = std::pin::pin!(f);

        loop {
            match f.as_mut().poll(&mut cx) {
                Poll::Ready(result) => return result,
                Poll::Pending => {
                    std::thread::yield_now();
                }
            }
        }
    }

    #[test]
    fn test_awaitable_just() {
        let result = block_on(just(42).into_awaitable());
        assert_eq!(result, Ok(42));
    }

    #[test]
    fn test_awaitable_string() {
        let result = block_on(just("hello".to_string()).into_awaitable());
        assert_eq!(result, Ok("hello".to_string()));
    }
}
