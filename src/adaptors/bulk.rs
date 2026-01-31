//! Bulk adaptor - execute a function for each index in a range.
//!
//! `bulk(sender, shape, fn)` calls the function with each index and a mutable
//! reference to the sender's value, then completes with the (possibly modified) value.

use crate::traits::{
    CompletionSignatures, Environment, OperationState, Receiver, ReceiverOf, ReceiverOfError,
    ReceiverOfStopped, Sender, SenderTo,
};
use std::marker::PhantomData;
use std::pin::Pin;

/// Execute a function for each index in a range.
///
/// When the upstream sender completes with a value, the function is called
/// with each index from 0 to shape (exclusive) and a mutable reference to the value.
/// After all iterations complete, the sender completes with the (possibly modified) value.
///
/// # Example
///
/// ```rust
/// use exec_rs::adaptors::bulk;
/// use exec_rs::factories::just;
/// use exec_rs::consumers::sync_wait;
///
/// // Accumulate sum of indices into the value
/// let sender = bulk(just(0usize), 5, |i, val| {
///     *val += i;
/// });
///
/// let result = sync_wait(sender).unwrap();
/// // 0 + 0 + 1 + 2 + 3 + 4 = 10
/// assert_eq!(result, 10);
/// ```
pub fn bulk<S, F, V, E>(sender: S, shape: usize, func: F) -> Bulk<S, F>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
    F: FnMut(usize, &mut V),
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

impl<S, F> Bulk<S, F> {
    /// Create a new Bulk adaptor.
    pub fn new(sender: S, shape: usize, func: F) -> Self {
        Self {
            sender,
            shape,
            func,
        }
    }
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
    F: FnMut(usize, &mut V) + Send + 'static,
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
    F: FnMut(usize, &mut V),
    R: ReceiverOf<V>,
{
    fn set_value(mut self, mut value: V) {
        // Execute the function for each index
        for i in 0..self.shape {
            (self.func)(i, &mut value);
        }
        // Forward the (possibly modified) value downstream
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
    use crate::traits::Sender;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    // ========================================================================
    // test_bulk.cpp equivalents
    // ========================================================================

    #[test]
    fn test_bulk_basic() {
        // Accumulate sum of indices multiplied by initial value
        let sender = bulk(just(10usize), 5, |i, val| {
            *val += i * 10; // Add i*10 to the value
        });

        let result = sync_wait(sender).unwrap();
        // 10 + 0*10 + 1*10 + 2*10 + 3*10 + 4*10 = 10 + 100 = 110
        assert_eq!(result, 110);
    }

    #[test]
    fn test_bulk_zero_shape() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let sender = bulk(just(42), 0, move |_, _: &mut i32| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        });

        let result = sync_wait(sender).unwrap();
        assert_eq!(result, 42);
        // Function should not be called
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_bulk_with_mutation() {
        // Collect indices into a vector
        let sender = bulk(just(Vec::<usize>::new()), 3, |i, vec| {
            vec.push(i);
        });

        let result = sync_wait(sender).unwrap();
        assert_eq!(result, vec![0, 1, 2]);
    }

    #[test]
    fn test_bulk_with_unit_value() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let sender = bulk(just(()), 10, move |_, _: &mut ()| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        });

        let result = sync_wait(sender).unwrap();
        assert_eq!(result, ());
        assert_eq!(counter.load(Ordering::SeqCst), 10);
    }

    #[test]
    fn test_bulk_is_sender() {
        fn assert_sender<S: Sender>(_: &S) {}
        let s = bulk(just(()), 5, |_, _| {});
        assert_sender(&s);
    }

    #[test]
    fn test_bulk_with_index() {
        // Accumulate indices into the value
        let sender = bulk(just(0usize), 5, |i, sum| {
            *sum += i;
        });

        let result = sync_wait(sender).unwrap();
        // 0 + 0 + 1 + 2 + 3 + 4 = 10
        assert_eq!(result, 10);
    }

    #[test]
    fn test_bulk_large_shape() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        sync_wait(bulk(just(()), 1000, move |_, _| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        }))
        .unwrap();

        assert_eq!(counter.load(Ordering::SeqCst), 1000);
    }

    #[test]
    fn test_bulk_preserves_value_when_not_modified() {
        let sender = bulk(just(42), 5, |_, _| {});
        let result = sync_wait(sender).unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn test_bulk_accumulate_sum() {
        // Classic use case: accumulate sum of indices
        let sender = bulk(just(0usize), 10, |i, sum| {
            *sum += i;
        });

        let result = sync_wait(sender).unwrap();
        // 0 + 1 + 2 + ... + 9 = 45
        assert_eq!(result, 45);
    }
}
