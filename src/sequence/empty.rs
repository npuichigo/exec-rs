//! Empty sequence sender - a sequence that emits no items.

use super::traits::{EmptyItemTypes, SequenceReceiver, SequenceSender, SequenceSenderTo};
use crate::traits::{OperationState, ReceiverOf, ValueCompletion};
use std::marker::PhantomData;
use std::pin::Pin;

/// Creates an empty sequence that completes immediately without emitting any items.
pub fn empty_sequence() -> EmptySequence {
    EmptySequence
}

/// An empty sequence sender.
#[derive(Clone, Copy, Debug, Default)]
pub struct EmptySequence;

impl SequenceSender for EmptySequence {
    type ItemTypes = EmptyItemTypes;
    type Completions = ValueCompletion<()>;
}

impl<R: SequenceReceiver + ReceiverOf<()>> SequenceSenderTo<R> for EmptySequence {
    type Operation = EmptySequenceOperation<R>;

    fn subscribe(self, receiver: R) -> Self::Operation {
        EmptySequenceOperation {
            receiver: Some(receiver),
            _marker: PhantomData,
        }
    }
}

/// Operation state for empty sequence.
pub struct EmptySequenceOperation<R> {
    receiver: Option<R>,
    _marker: PhantomData<R>,
}

impl<R: ReceiverOf<()>> OperationState for EmptySequenceOperation<R> {
    fn start(self: Pin<&mut Self>) {
        // Empty sequence immediately completes with set_value()
        let this = unsafe { self.get_unchecked_mut() };
        if let Some(receiver) = this.receiver.take() {
            receiver.set_value(());
        }
    }
}
