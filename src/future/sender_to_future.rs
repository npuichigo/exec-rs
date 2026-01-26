//! Convert a Sender to a std::future::Future.
//!
//! This allows exec-rs senders to be used in async/await contexts.

use crate::stop::NeverStopToken;
use crate::traits::{
    CompletionSignatures, Environment, OperationState, Receiver, ReceiverOf, ReceiverOfError,
    ReceiverOfStopped, Sender, SenderTo,
};
use parking_lot::Mutex;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

/// Extension trait to convert a sender to a future.
pub trait SenderToFutureExt: Sender {
    /// Convert this sender to a Future.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use exec_rs::factories::just;
    /// use exec_rs::future::SenderToFutureExt;
    ///
    /// async fn example() {
    ///     let future = just(42).into_future();
    ///     let result = future.await;
    ///     assert_eq!(result, Ok(42));
    /// }
    /// ```
    fn into_future(self) -> SenderFuture<Self>
    where
        Self: Sized,
    {
        SenderFuture::new(self)
    }
}

impl<S: Sender> SenderToFutureExt for S {}

/// Error type for sender futures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SenderFutureError<E> {
    /// The sender completed with an error.
    Error(E),
    /// The sender was stopped/cancelled.
    Stopped,
}

/// A Future that wraps a Sender.
///
/// When polled, this future starts the sender and waits for completion.
pub struct SenderFuture<S>
where
    S: Sender,
{
    state: SenderFutureState<S>,
}

enum SenderFutureState<S: Sender> {
    /// Sender not yet started
    Pending(Option<S>),
    /// Sender is running
    Running {
        inner: Arc<
            SenderFutureInner<
                <S::Completions as CompletionSignatures>::Value,
                <S::Completions as CompletionSignatures>::Error,
            >,
        >,
        _operation: Option<Box<dyn std::any::Any + Send>>,
    },
    /// Completed
    Done,
}

struct SenderFutureInner<V, E> {
    result: Mutex<Option<Result<V, SenderFutureError<E>>>>,
    waker: Mutex<Option<Waker>>,
}

impl<S: Sender> SenderFuture<S> {
    /// Create a new SenderFuture wrapping the given sender.
    pub fn new(sender: S) -> Self {
        Self {
            state: SenderFutureState::Pending(Some(sender)),
        }
    }
}

impl<S, V, E> Future for SenderFuture<S>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
    S: SenderTo<FutureReceiver<V, E>>,
    V: Send + 'static,
    E: Send + 'static,
    S::Operation: Send + 'static,
{
    type Output = Result<V, SenderFutureError<E>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };

        loop {
            match &mut this.state {
                SenderFutureState::Pending(sender_opt) => {
                    let sender = sender_opt.take().expect("Polled after completion");

                    let inner: Arc<SenderFutureInner<V, E>> = Arc::new(SenderFutureInner {
                        result: Mutex::new(None),
                        waker: Mutex::new(Some(cx.waker().clone())),
                    });

                    let receiver = FutureReceiver {
                        inner: inner.clone(),
                    };

                    let operation = sender.connect(receiver);

                    // Box the operation to erase its type
                    let mut boxed_op: Pin<Box<dyn OperationStateBoxed + Send>> =
                        Box::pin(OperationWrapper(operation));

                    // Start the operation
                    boxed_op.as_mut().start_boxed();

                    this.state = SenderFutureState::Running {
                        inner,
                        _operation: Some(Box::new(boxed_op)),
                    };
                }
                SenderFutureState::Running { inner, .. } => {
                    // Update the waker
                    *inner.waker.lock() = Some(cx.waker().clone());

                    // Check if result is ready
                    let result = inner.result.lock().take();
                    if let Some(result) = result {
                        this.state = SenderFutureState::Done;
                        return Poll::Ready(result);
                    }

                    return Poll::Pending;
                }
                SenderFutureState::Done => {
                    panic!("SenderFuture polled after completion");
                }
            }
        }
    }
}

// Type-erased OperationState for boxing
trait OperationStateBoxed {
    fn start_boxed(self: Pin<&mut Self>);
}

struct OperationWrapper<Op>(Op);

impl<Op: OperationState> OperationStateBoxed for OperationWrapper<Op> {
    fn start_boxed(self: Pin<&mut Self>) {
        unsafe {
            let this = self.get_unchecked_mut();
            Pin::new_unchecked(&mut this.0).start();
        }
    }
}

/// Receiver that completes a future.
pub struct FutureReceiver<V, E> {
    inner: Arc<SenderFutureInner<V, E>>,
}

/// Environment for FutureReceiver.
pub struct FutureReceiverEnv;

impl Environment for FutureReceiverEnv {
    type StopToken = NeverStopToken;

    fn get_stop_token(&self) -> Self::StopToken {
        NeverStopToken
    }
}

impl<V, E> Receiver for FutureReceiver<V, E> {
    type Env = FutureReceiverEnv;

    fn get_env(&self) -> &Self::Env {
        static ENV: FutureReceiverEnv = FutureReceiverEnv;
        &ENV
    }
}

impl<V: Send + 'static, E> ReceiverOf<V> for FutureReceiver<V, E> {
    fn set_value(self, value: V) {
        *self.inner.result.lock() = Some(Ok(value));
        if let Some(waker) = self.inner.waker.lock().take() {
            waker.wake();
        }
    }
}

impl<V, E: Send + 'static> ReceiverOfError<E> for FutureReceiver<V, E> {
    fn set_error(self, error: E) {
        *self.inner.result.lock() = Some(Err(SenderFutureError::Error(error)));
        if let Some(waker) = self.inner.waker.lock().take() {
            waker.wake();
        }
    }
}

impl<V, E: Send + 'static> ReceiverOfStopped for FutureReceiver<V, E> {
    fn set_stopped(self) {
        *self.inner.result.lock() = Some(Err(SenderFutureError::Stopped));
        if let Some(waker) = self.inner.waker.lock().take() {
            waker.wake();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::factories::{just, just_error, just_stopped};

    fn block_on<F: Future>(f: F) -> F::Output {
        use std::task::{RawWaker, RawWakerVTable};

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
    fn test_sender_to_future_value() {
        let future = just(42).into_future();
        let result = block_on(future);
        assert_eq!(result, Ok(42));
    }

    #[test]
    fn test_sender_to_future_error() {
        let future = just_error("error").into_future();
        let result = block_on(future);
        assert_eq!(result, Err(SenderFutureError::Error("error")));
    }

    #[test]
    fn test_sender_to_future_stopped() {
        let future = just_stopped().into_future();
        let result = block_on(future);
        assert_eq!(result, Err(SenderFutureError::Stopped));
    }
}
