//! Bulk adaptor - execute a function for each index in a range.
//!
//! `bulk(sender, shape, fn)` calls the function with each index and the
//! sender's value, then completes with the original value.

use crate::traits::{
    CompletionSignatures, Environment, OperationState, Receiver, ReceiverOf, ReceiverOfError,
    ReceiverOfStopped, Sender, SenderTo,
};
use std::marker::PhantomData;
use std::pin::Pin;

/// Execute a function for each index in a range.
///
/// When the upstream sender completes with a value, the function is called
/// with each index from 0 to shape (exclusive) and a reference to the value.
/// After all iterations complete, the sender completes with the original value.
///
/// # Example
///
/// ```rust
/// use exec_rs::adaptors::bulk;
/// use exec_rs::factories::just;
/// use exec_rs::consumers::sync_wait;
/// use std::sync::atomic::{AtomicUsize, Ordering};
/// use std::sync::Arc;
///
/// let sum = Arc::new(AtomicUsize::new(0));
/// let sum_clone = sum.clone();
///
/// let sender = bulk(just(10), 5, move |i, &val| {
///     sum_clone.fetch_add(i * val, Ordering::SeqCst);
/// });
///
/// let result = sync_wait(sender).unwrap();
/// assert_eq!(result, 10);
/// // sum = 0*10 + 1*10 + 2*10 + 3*10 + 4*10 = 100
/// assert_eq!(sum.load(Ordering::SeqCst), 100);
/// ```
pub fn bulk<S, F, V, E>(sender: S, shape: usize, func: F) -> Bulk<S, F>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
    F: FnMut(usize, &V),
{
    Bulk {
        sender,
        shape,
        func,
    }
}

/// A sender that executes a function for each index in a range.
pub struct Bulk<S, F> {
    sender: S,
    shape: usize,
    func: F,
}

impl<S: Clone, F: Clone> Clone for Bulk<S, F> {
    fn clone(&self) -> Self {
        Bulk {
            sender: self.sender.clone(),
            shape: self.shape,
            func: self.func.clone(),
        }
    }
}

impl<S, F> std::fmt::Debug for Bulk<S, F>
where
    S: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bulk")
            .field("sender", &self.sender)
            .field("shape", &self.shape)
            .finish()
    }
}

/// Completion signatures for bulk - same as upstream sender.
pub struct BulkCompletions<V, E> {
    _phantom: PhantomData<(V, E)>,
}

impl<V, E> CompletionSignatures for BulkCompletions<V, E> {
    type Value = V;
    type Error = E;
    const SENDS_STOPPED: bool = true;
}

impl<S, F, V, E> Sender for Bulk<S, F>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
{
    type Completions = BulkCompletions<V, E>;
}

impl<S, F, V, E, R> SenderTo<R> for Bulk<S, F>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>
        + SenderTo<BulkReceiver<V, E, F, R>>,
    F: FnMut(usize, &V) + Send + 'static,
    V: Send + 'static,
    E: Send + 'static,
    R: ReceiverOf<V> + ReceiverOfError<E> + ReceiverOfStopped + Send + 'static,
{
    type Operation = BulkOperation<<S as SenderTo<BulkReceiver<V, E, F, R>>>::Operation>;

    fn connect(self, receiver: R) -> Self::Operation {
        let inner_receiver = BulkReceiver {
            shape: self.shape,
            func: self.func,
            downstream: receiver,
            _phantom: PhantomData,
        };
        BulkOperation {
            inner: self.sender.connect(inner_receiver),
        }
    }
}

/// Operation state for bulk.
pub struct BulkOperation<Op> {
    inner: Op,
}

impl<Op: OperationState> OperationState for BulkOperation<Op> {
    fn start(self: Pin<&mut Self>) {
        unsafe {
            let this = self.get_unchecked_mut();
            Pin::new_unchecked(&mut this.inner).start();
        }
    }
}

/// Environment for bulk receiver.
pub struct BulkEnv;

impl Environment for BulkEnv {
    type StopToken = crate::stop::NeverStopToken;

    fn get_stop_token(&self) -> Self::StopToken {
        crate::stop::NeverStopToken
    }
}

/// Receiver that executes the bulk function on value completion.
pub struct BulkReceiver<V, E, F, R> {
    shape: usize,
    func: F,
    downstream: R,
    _phantom: PhantomData<(V, E)>,
}

impl<V, E, F, R> Receiver for BulkReceiver<V, E, F, R> {
    type Env = BulkEnv;

    fn get_env(&self) -> &Self::Env {
        &BulkEnv
    }
}

impl<V, E, F, R> ReceiverOf<V> for BulkReceiver<V, E, F, R>
where
    F: FnMut(usize, &V),
    R: ReceiverOf<V>,
{
    fn set_value(mut self, value: V) {
        // Execute the function for each index
        for i in 0..self.shape {
            (self.func)(i, &value);
        }
        // Forward the value downstream
        self.downstream.set_value(value);
    }
}

impl<V, E, F, R> ReceiverOfError<E> for BulkReceiver<V, E, F, R>
where
    R: ReceiverOfError<E>,
{
    fn set_error(self, error: E) {
        self.downstream.set_error(error);
    }
}

impl<V, E, F, R> ReceiverOfStopped for BulkReceiver<V, E, F, R>
where
    R: ReceiverOfStopped,
{
    fn set_stopped(self) {
        self.downstream.set_stopped();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consumers::sync_wait;
    use crate::factories::just;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn test_bulk_basic() {
        let sum = Arc::new(AtomicUsize::new(0));
        let sum_clone = sum.clone();

        let sender = bulk(just(10usize), 5, move |i, &val| {
            sum_clone.fetch_add(i * val, Ordering::SeqCst);
        });

        let result = sync_wait(sender).unwrap();
        assert_eq!(result, 10);
        // sum = 0*10 + 1*10 + 2*10 + 3*10 + 4*10 = 100
        assert_eq!(sum.load(Ordering::SeqCst), 100);
    }

    #[test]
    fn test_bulk_zero_shape() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let sender = bulk(just(42), 0, move |_, _: &i32| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        });

        let result = sync_wait(sender).unwrap();
        assert_eq!(result, 42);
        // Function should not be called
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_bulk_with_mutation() {
        let results = Arc::new(std::sync::Mutex::new(Vec::new()));
        let results_clone = results.clone();

        let sender = bulk(just("hello"), 3, move |i, &val| {
            results_clone.lock().unwrap().push((i, val.to_string()));
        });

        let result = sync_wait(sender).unwrap();
        assert_eq!(result, "hello");

        let collected = results.lock().unwrap();
        assert_eq!(collected.len(), 3);
        assert_eq!(collected[0], (0, "hello".to_string()));
        assert_eq!(collected[1], (1, "hello".to_string()));
        assert_eq!(collected[2], (2, "hello".to_string()));
    }

    #[test]
    fn test_bulk_with_unit_value() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let sender = bulk(just(()), 10, move |_, _: &()| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        });

        let result = sync_wait(sender).unwrap();
        assert_eq!(result, ());
        assert_eq!(counter.load(Ordering::SeqCst), 10);
    }
}
