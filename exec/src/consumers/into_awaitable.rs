use exec_core::receiver::{SetError, SetValue};
use exec_core::{OperationState, Sender};
use std::cell::UnsafeCell;
use std::error::Error;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

#[allow(dead_code)]
#[derive(Debug)]
pub enum AwaitResult<V, E> {
    Value(V),
    Error(E),
    Stopped,
}

struct SharedState<V, E> {
    result: Option<AwaitResult<V, E>>,
    waker: Option<Waker>,
}

pub struct Receiver<V, E> {
    state: Rc<UnsafeCell<SharedState<V, E>>>,
}

impl<V, E> SetValue for Receiver<V, E> {
    type Value = V;

    fn set_value(self, value: Self::Value) {
        unsafe {
            let _ = (*self.state.get()).result.insert(AwaitResult::Value(value));
            if let Some(waker) = (*self.state.get()).waker.take() {
                waker.wake();
            }
        }
    }
}

impl<V, E: Error> SetError for Receiver<V, E> {
    type Error = E;

    fn set_error(self, error: Self::Error) {
        unsafe {
            let _ = (*self.state.get()).result.insert(AwaitResult::Error(error));
            if let Some(waker) = (*self.state.get()).waker.take() {
                waker.wake();
            }
        }
    }
}

pub struct SenderAwaitable<S, V, E>
where
    S: Sender<Receiver<V, E>, Value = V, Error = E>,
{
    state: Rc<UnsafeCell<SharedState<V, E>>>,
    operation: S::Operation,
}

impl<S, V, E> SenderAwaitable<S, V, E>
where
    S: Sender<Receiver<V, E>, Value = V, Error = E>,
{
    pub(crate) fn new(sender: S) -> Self {
        let state = Rc::new(UnsafeCell::new(SharedState {
            result: None,
            waker: None,
        }));

        let receiver = Receiver {
            state: state.clone(),
        };

        Self {
            state,
            operation: sender.connect(receiver),
        }
    }
}

impl<S, V, E> Future for SenderAwaitable<S, V, E>
where
    S: Sender<Receiver<V, E>, Value = V, Error = E>,
{
    type Output = Result<Option<V>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let me = unsafe { self.get_unchecked_mut() };

        if let Some(result) = unsafe { (*me.state.get()).result.take() } {
            match result {
                AwaitResult::Value(value) => Poll::Ready(Ok(Some(value))),
                AwaitResult::Error(error) => Poll::Ready(Err(error)),
                AwaitResult::Stopped => Poll::Ready(Ok(None)),
            }
        } else {
            unsafe {
                let _ = (*me.state.get()).waker.insert(cx.waker().clone());
            }
            me.operation.start();
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::consumers::SenderAwaitable;
    use crate::factories::just_error;
    use crate::{just, then};
    use exec_test::errors::TestError;
    use futures::executor::block_on;

    #[test]
    fn test_awaitable() {
        block_on(async {
            let sender = just(1);
            let sender = then(sender, |x| x + 1);
            let awaitable = SenderAwaitable::new(sender);
            println!("{:?}", awaitable.await);
        });
    }

    #[test]
    fn test_awaitable_error() {
        block_on(async {
            let sender = just_error(TestError);
            let awaitable = SenderAwaitable::new(sender);
            println!("{:?}", awaitable.await);
        });
    }
}
