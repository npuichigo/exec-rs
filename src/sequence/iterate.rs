//! Iterate sequence sender - converts an iterator into a sequence sender.

use super::traits::{
    ItemTypes, JustNextSender, SequenceReceiver, SequenceSender, SequenceSenderTo,
};
use crate::schedulers::trampoline_submit;
use crate::traits::{OperationState, ReceiverOf, Sender, SenderTo, ValueCompletion};
use std::cell::UnsafeCell;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;

/// Creates a sequence sender from an iterator.
///
/// Each item from the iterator is wrapped in a sender and emitted
/// to the sequence receiver via `set_next`.
pub fn iterate<I>(iter: I) -> Iterate<I::IntoIter>
where
    I: IntoIterator,
{
    Iterate {
        iter: iter.into_iter(),
    }
}

/// A sequence sender created from an iterator.
pub struct Iterate<I> {
    iter: I,
}

/// Item types for iterate - each item is wrapped in JustItem.
pub struct SingleItemType<S>(PhantomData<S>);

impl<S> Default for SingleItemType<S> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<S: Sender> ItemTypes for SingleItemType<S> {
    fn for_each<F: super::traits::ItemTypeVisitor>(&self, mut visitor: F) {
        visitor.visit::<S>();
    }
}

impl<I: Iterator> SequenceSender for Iterate<I>
where
    I::Item: 'static,
{
    type ItemTypes = SingleItemType<JustItem<I::Item>>;
    type Completions = ValueCompletion<()>;
}

/// A sender that just produces a single value.
pub struct JustItem<T> {
    value: T,
}

impl<T> JustItem<T> {
    /// Create a new JustItem sender.
    pub fn new(value: T) -> Self {
        Self { value }
    }
}

impl<T: 'static> Sender for JustItem<T> {
    type Completions = ValueCompletion<T>;
}

impl<T: 'static, R: ReceiverOf<T>> SenderTo<R> for JustItem<T> {
    type Operation = JustItemOperation<T, R>;

    fn connect(self, receiver: R) -> Self::Operation {
        JustItemOperation {
            value: Some(self.value),
            receiver: Some(receiver),
        }
    }
}

/// Operation for JustItem.
pub struct JustItemOperation<T, R> {
    value: Option<T>,
    receiver: Option<R>,
}

impl<T, R: ReceiverOf<T>> OperationState for JustItemOperation<T, R> {
    fn start(self: Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };
        if let (Some(value), Some(receiver)) = (this.value.take(), this.receiver.take()) {
            receiver.set_value(value);
        }
    }
}

// ============================================================================
// Simplified implementation that works with JustNextSender
// ============================================================================

/// Internal state for iterate operation.
struct IterateState<I: Iterator, R> {
    iter: UnsafeCell<I>,
    receiver: UnsafeCell<Option<R>>,
    next_op: UnsafeCell<Option<JustNextOperation>>,
}

// Safety: We ensure single-threaded access through the trampoline
unsafe impl<I: Iterator + Send, R: Send> Send for IterateState<I, R> {}
unsafe impl<I: Iterator + Send, R: Send> Sync for IterateState<I, R> {}

/// Simplified next operation that doesn't need type parameters.
struct JustNextOperation {
    continue_: bool,
}

impl JustNextOperation {
    fn start(&mut self) -> bool {
        self.continue_
    }
}

/// Operation state for iterate.
pub struct IterateOperation<I: Iterator, R> {
    state: Arc<IterateState<I, R>>,
}

impl<I, R> SequenceSenderTo<R> for Iterate<I>
where
    I: Iterator + 'static,
    I::Item: 'static,
    R: SequenceReceiver<NextSender = JustNextSender> + ReceiverOf<()> + 'static,
{
    type Operation = IterateOperation<I, R>;

    fn subscribe(self, receiver: R) -> Self::Operation {
        IterateOperation {
            state: Arc::new(IterateState {
                iter: UnsafeCell::new(self.iter),
                receiver: UnsafeCell::new(Some(receiver)),
                next_op: UnsafeCell::new(None),
            }),
        }
    }
}

impl<I, R> IterateOperation<I, R>
where
    I: Iterator + 'static,
    I::Item: 'static,
    R: SequenceReceiver<NextSender = JustNextSender> + ReceiverOf<()> + 'static,
{
    fn start_next(state: Arc<IterateState<I, R>>) {
        // Safety: We're the only ones accessing these cells due to sequential execution
        let iter = unsafe { &mut *state.iter.get() };
        let receiver_opt = unsafe { &mut *state.receiver.get() };

        match iter.next() {
            Some(item) => {
                // Emit item via set_next
                if let Some(receiver) = receiver_opt.as_mut() {
                    let item_sender = JustItem::new(item);
                    let next_sender = receiver.set_next(item_sender);

                    // Store the result (continue or stop)
                    unsafe {
                        *state.next_op.get() = Some(JustNextOperation {
                            continue_: next_sender.continue_,
                        });
                    }

                    // Process via trampoline to prevent stack overflow
                    let state_clone = state.clone();
                    trampoline_submit(move || {
                        let next_op = unsafe { &mut *state_clone.next_op.get() };
                        if let Some(ref mut op) = next_op {
                            if op.start() {
                                // Continue with next item
                                Self::start_next(state_clone.clone());
                            } else {
                                // Stop requested - complete sequence
                                let receiver_opt = unsafe { &mut *state_clone.receiver.get() };
                                if let Some(receiver) = receiver_opt.take() {
                                    receiver.set_value(());
                                }
                            }
                        }
                    });
                }
            }
            None => {
                // Iterator exhausted, complete the sequence
                if let Some(receiver) = receiver_opt.take() {
                    receiver.set_value(());
                }
            }
        }
    }
}

impl<I, R> OperationState for IterateOperation<I, R>
where
    I: Iterator + 'static,
    I::Item: 'static,
    R: SequenceReceiver<NextSender = JustNextSender> + ReceiverOf<()> + 'static,
{
    fn start(self: Pin<&mut Self>) {
        let state = self.state.clone();
        Self::start_next(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::{Receiver, ReceiverOf, ReceiverOfStopped, Sender};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    struct CountingReceiver {
        count: Arc<AtomicUsize>,
        completed: Arc<AtomicBool>,
    }

    impl Receiver for CountingReceiver {
        type Env = ();

        fn get_env(&self) -> &Self::Env {
            &()
        }
    }

    impl ReceiverOf<()> for CountingReceiver {
        fn set_value(self, _: ()) {
            self.completed.store(true, Ordering::SeqCst);
        }
    }

    impl ReceiverOfStopped for CountingReceiver {
        fn set_stopped(self) {}
    }

    impl SequenceReceiver for CountingReceiver {
        type NextSender = JustNextSender;

        fn set_next<S: Sender>(&mut self, _item: S) -> Self::NextSender {
            self.count.fetch_add(1, Ordering::SeqCst);
            JustNextSender::continue_()
        }
    }

    #[test]
    fn test_iterate_basic() {
        let count = Arc::new(AtomicUsize::new(0));
        let completed = Arc::new(AtomicBool::new(false));

        let seq = iterate(0..5);
        let receiver = CountingReceiver {
            count: count.clone(),
            completed: completed.clone(),
        };

        let mut op = seq.subscribe(receiver);
        let pinned = unsafe { Pin::new_unchecked(&mut op) };
        pinned.start();

        assert_eq!(count.load(Ordering::SeqCst), 5);
        assert!(completed.load(Ordering::SeqCst));
    }

    #[test]
    fn test_iterate_empty() {
        let count = Arc::new(AtomicUsize::new(0));
        let completed = Arc::new(AtomicBool::new(false));

        let seq = iterate(std::iter::empty::<i32>());
        let receiver = CountingReceiver {
            count: count.clone(),
            completed: completed.clone(),
        };

        let mut op = seq.subscribe(receiver);
        let pinned = unsafe { Pin::new_unchecked(&mut op) };
        pinned.start();

        assert_eq!(count.load(Ordering::SeqCst), 0);
        assert!(completed.load(Ordering::SeqCst));
    }

    #[test]
    fn test_iterate_large() {
        // Test with many items to verify trampoline prevents stack overflow
        let count = Arc::new(AtomicUsize::new(0));
        let completed = Arc::new(AtomicBool::new(false));

        let seq = iterate(0..10000);
        let receiver = CountingReceiver {
            count: count.clone(),
            completed: completed.clone(),
        };

        let mut op = seq.subscribe(receiver);
        let pinned = unsafe { Pin::new_unchecked(&mut op) };
        pinned.start();

        assert_eq!(count.load(Ordering::SeqCst), 10000);
        assert!(completed.load(Ordering::SeqCst));
    }
}
