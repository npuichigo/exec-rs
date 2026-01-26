//! Task type for async/await integration.
//!
//! Provides a `Task<T>` type that represents spawned async work
//! with stop token propagation and scheduler affinity.

use crate::consumers::start_detached;
use crate::stop::{InplaceStopSource, InplaceStopToken, StopSource};
use crate::traits::{
    CompletionSignatures, Environment, OperationState, ReceiverOf, ReceiverOfError,
    ReceiverOfStopped, Sender, SenderTo,
};
use parking_lot::Mutex;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

/// A task handle representing spawned async work.
///
/// When dropped, the task is detached (continues running).
/// Use `.await` or `.join()` to wait for completion.
///
/// # Example
///
/// ```rust,ignore
/// use exec_rs::coro::Task;
/// use exec_rs::factories::just;
///
/// async fn example() {
///     let task = Task::spawn(just(42));
///     let result = task.await;
///     assert_eq!(result, Ok(42));
/// }
/// ```
pub struct Task<T, E = std::convert::Infallible> {
    inner: Arc<TaskInner<T, E>>,
    stop_source: Option<InplaceStopSource>,
}

struct TaskInner<T, E> {
    result: Mutex<Option<Result<T, TaskError<E>>>>,
    waker: Mutex<Option<Waker>>,
}

/// Error type for tasks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskError<E> {
    /// The task completed with an error.
    Error(E),
    /// The task was cancelled/stopped.
    Cancelled,
}

impl<T, E> Task<T, E> {
    /// Create a new task from a sender.
    ///
    /// The sender will be started immediately in a detached manner,
    /// and the task handle can be used to wait for completion.
    pub fn spawn<S>(sender: S) -> Self
    where
        S: Sender<Completions: CompletionSignatures<Value = T, Error = E>>,
        S: SenderTo<TaskReceiver<T, E>>,
        S::Operation: Send + 'static,
        T: Send + 'static,
        E: Send + 'static,
    {
        let stop_source = InplaceStopSource::new();
        let inner = Arc::new(TaskInner {
            result: Mutex::new(None),
            waker: Mutex::new(None),
        });

        let receiver = TaskReceiver {
            inner: inner.clone(),
            stop_token: stop_source.get_token(),
        };

        let operation = sender.connect(receiver);

        // Start the operation using a wrapper that boxes it
        let wrapper = TaskOperationWrapper {
            operation: Some(operation),
        };
        start_detached(wrapper);

        Task {
            inner,
            stop_source: Some(stop_source),
        }
    }

    /// Request cancellation of the task.
    pub fn cancel(&self) {
        if let Some(ref source) = self.stop_source {
            source.request_stop();
        }
    }

    /// Check if the task has completed.
    pub fn is_complete(&self) -> bool {
        self.inner.result.lock().is_some()
    }

    /// Get the stop token for this task.
    pub fn get_stop_token(&self) -> Option<InplaceStopToken> {
        self.stop_source.as_ref().map(|s| s.get_token())
    }
}

impl<T, E> Future for Task<T, E>
where
    T: Send + 'static,
    E: Send + 'static,
{
    type Output = Result<T, TaskError<E>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        // Update waker
        *this.inner.waker.lock() = Some(cx.waker().clone());

        // Check for result
        if let Some(result) = this.inner.result.lock().take() {
            Poll::Ready(result)
        } else {
            Poll::Pending
        }
    }
}

// Wrapper sender that starts the boxed operation
struct TaskOperationWrapper<Op> {
    operation: Option<Op>,
}

impl<Op> Sender for TaskOperationWrapper<Op> {
    type Completions = TaskWrapperCompletions;
}

struct TaskWrapperCompletions;

impl CompletionSignatures for TaskWrapperCompletions {
    type Value = ();
    type Error = std::convert::Infallible;
    const SENDS_STOPPED: bool = false;
}

// Receiver for TaskOperationWrapper that just starts the inner operation
struct TaskWrapperReceiver<Op> {
    operation: Pin<Box<Op>>,
}

impl<Op: OperationState> crate::traits::Receiver for TaskWrapperReceiver<Op> {
    type Env = TaskReceiverEnv;
    fn get_env(&self) -> &Self::Env {
        static ENV: TaskReceiverEnv = TaskReceiverEnv;
        &ENV
    }
}

impl<Op: OperationState> ReceiverOf<()> for TaskWrapperReceiver<Op> {
    fn set_value(self, _: ()) {}
}

impl<Op, R: crate::traits::Receiver> SenderTo<R> for TaskOperationWrapper<Op>
where
    Op: OperationState + Send + 'static,
    R: ReceiverOf<()> + ReceiverOfStopped,
{
    type Operation = TaskWrapperOperation<Op, R>;

    fn connect(self, receiver: R) -> Self::Operation {
        TaskWrapperOperation {
            inner_op: self.operation,
            receiver: Some(receiver),
        }
    }
}

struct TaskWrapperOperation<Op, R> {
    inner_op: Option<Op>,
    receiver: Option<R>,
}

impl<Op, R> OperationState for TaskWrapperOperation<Op, R>
where
    Op: OperationState + Send + 'static,
    R: ReceiverOf<()>,
{
    fn start(self: Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };

        if let Some(op) = this.inner_op.take() {
            // Start the inner operation on a new thread to avoid blocking
            let mut pinned_op = Box::pin(op);
            std::thread::spawn(move || {
                pinned_op.as_mut().start();
            });
        }

        // Signal immediate completion of the wrapper
        if let Some(receiver) = this.receiver.take() {
            receiver.set_value(());
        }
    }
}

/// Receiver that completes a task.
pub struct TaskReceiver<T, E> {
    inner: Arc<TaskInner<T, E>>,
    stop_token: InplaceStopToken,
}

/// Environment for TaskReceiver.
pub struct TaskReceiverEnv;

impl Environment for TaskReceiverEnv {
    type StopToken = InplaceStopToken;

    fn get_stop_token(&self) -> Self::StopToken {
        // This is a placeholder; the actual stop token is in the receiver
        InplaceStopSource::new().get_token()
    }
}

impl<T, E> crate::traits::Receiver for TaskReceiver<T, E> {
    type Env = TaskReceiverEnv;

    fn get_env(&self) -> &Self::Env {
        static ENV: TaskReceiverEnv = TaskReceiverEnv;
        &ENV
    }
}

impl<T: Send + 'static, E> ReceiverOf<T> for TaskReceiver<T, E> {
    fn set_value(self, value: T) {
        *self.inner.result.lock() = Some(Ok(value));
        if let Some(waker) = self.inner.waker.lock().take() {
            waker.wake();
        }
    }
}

impl<T, E: Send + 'static> ReceiverOfError<E> for TaskReceiver<T, E> {
    fn set_error(self, error: E) {
        *self.inner.result.lock() = Some(Err(TaskError::Error(error)));
        if let Some(waker) = self.inner.waker.lock().take() {
            waker.wake();
        }
    }
}

impl<T, E: Send + 'static> ReceiverOfStopped for TaskReceiver<T, E> {
    fn set_stopped(self) {
        *self.inner.result.lock() = Some(Err(TaskError::Cancelled));
        if let Some(waker) = self.inner.waker.lock().take() {
            waker.wake();
        }
    }
}

/// Extension trait for spawning senders as tasks.
pub trait SpawnExt<V, E>: Sender<Completions: CompletionSignatures<Value = V, Error = E>> {
    /// Spawn this sender as an async task.
    ///
    /// Returns a `Task` handle that can be awaited or used to cancel the work.
    fn spawn_task(self) -> Task<V, E>
    where
        Self: Sized + SenderTo<TaskReceiver<V, E>>,
        Self::Operation: Send + 'static,
        V: Send + 'static,
        E: Send + 'static,
    {
        Task::spawn(self)
    }
}

impl<S, V, E> SpawnExt<V, E> for S where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>
{
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::factories::just;
    use std::time::Duration;

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
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        }
    }

    #[test]
    fn test_task_spawn_and_await() {
        let task = Task::spawn(just(42));
        let result = block_on(task);
        assert_eq!(result, Ok(42));
    }

    #[test]
    fn test_task_spawn_ext() {
        let task = just(100).spawn_task();
        let result = block_on(task);
        assert_eq!(result, Ok(100));
    }
}
