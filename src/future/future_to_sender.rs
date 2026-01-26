//! Convert a std::future::Future to a Sender.
//!
//! This allows Rust futures to be used in exec-rs sender pipelines.

use crate::traits::{
    CompletionSignatures, OperationState, ReceiverOf, ReceiverOfStopped, Sender, SenderTo,
};
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

/// Convert a Future into a Sender.
///
/// The resulting sender, when started, will poll the future to completion
/// and deliver the result to the receiver.
///
/// # Example
///
/// ```rust
/// use exec_rs::future::from_future;
/// use exec_rs::consumers::sync_wait;
///
/// let sender = from_future(async { 42 });
/// let result = sync_wait(sender).unwrap();
/// assert_eq!(result, 42);
/// ```
pub fn from_future<F>(future: F) -> FutureSender<F>
where
    F: Future,
{
    FutureSender { future }
}

/// A Sender that wraps a Future.
pub struct FutureSender<F> {
    future: F,
}

impl<F> FutureSender<F> {
    /// Create a new FutureSender from a Future.
    pub fn new(future: F) -> Self {
        Self { future }
    }
}

/// Completion signatures for FutureSender.
pub struct FutureSenderCompletions<T> {
    _phantom: PhantomData<T>,
}

impl<T> CompletionSignatures for FutureSenderCompletions<T> {
    type Value = T;
    type Error = std::convert::Infallible;
    const SENDS_STOPPED: bool = false;
}

impl<F: Future> Sender for FutureSender<F> {
    type Completions = FutureSenderCompletions<F::Output>;
}

impl<F, R> SenderTo<R> for FutureSender<F>
where
    F: Future,
    R: ReceiverOf<F::Output> + ReceiverOfStopped,
{
    type Operation = FutureSenderOperation<F, R>;

    fn connect(self, receiver: R) -> Self::Operation {
        FutureSenderOperation {
            future: self.future,
            receiver: Some(receiver),
        }
    }
}

/// Operation state for FutureSender.
pub struct FutureSenderOperation<F, R> {
    future: F,
    receiver: Option<R>,
}

impl<F, R> OperationState for FutureSenderOperation<F, R>
where
    F: Future,
    R: ReceiverOf<F::Output>,
{
    fn start(self: Pin<&mut Self>) {
        // SAFETY: We don't move out of self
        let this = unsafe { self.get_unchecked_mut() };

        if let Some(receiver) = this.receiver.take() {
            // Create a no-op waker for synchronous polling
            let waker = noop_waker();
            let mut cx = Context::from_waker(&waker);

            // Poll the future in a loop until completion
            // SAFETY: We're pinning the future for the duration of this operation
            let future_ptr = &mut this.future as *mut F;

            loop {
                let future_pin = unsafe { Pin::new_unchecked(&mut *future_ptr) };

                match future_pin.poll(&mut cx) {
                    Poll::Ready(value) => {
                        receiver.set_value(value);
                        return;
                    }
                    Poll::Pending => {
                        // Yield and try again
                        // Note: This is a busy-wait; for production use,
                        // integrate with a proper async runtime
                        std::thread::yield_now();
                    }
                }
            }
        }
    }
}

/// Create a no-op waker for synchronous polling.
fn noop_waker() -> Waker {
    const VTABLE: RawWakerVTable = RawWakerVTable::new(
        |_| RawWaker::new(std::ptr::null(), &VTABLE), // clone
        |_| {},                                       // wake
        |_| {},                                       // wake_by_ref
        |_| {},                                       // drop
    );

    // SAFETY: The vtable functions are valid
    unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) }
}

/// Extension trait to convert a Future into a Sender.
pub trait FutureToSenderExt: Future + Sized {
    /// Convert this future into a sender.
    fn into_sender(self) -> FutureSender<Self> {
        FutureSender::new(self)
    }
}

impl<F: Future> FutureToSenderExt for F {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consumers::sync_wait;

    #[test]
    fn test_future_to_sender_immediate() {
        let sender = from_future(async { 42 });
        let result = sync_wait(sender).unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn test_future_to_sender_with_computation() {
        let sender = from_future(async {
            let a = 10;
            let b = 20;
            a + b
        });
        let result = sync_wait(sender).unwrap();
        assert_eq!(result, 30);
    }

    #[test]
    fn test_future_extension_trait() {
        let future = async { "hello" };
        let sender = future.into_sender();
        let result = sync_wait(sender).unwrap();
        assert_eq!(result, "hello");
    }
}
