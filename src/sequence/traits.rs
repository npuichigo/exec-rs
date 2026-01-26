//! Sequence sender traits and core types.

use crate::traits::{
    CompletionSignatures, OperationState, Receiver, ReceiverOf, ReceiverOfStopped, Sender,
    SenderTo, ValueCompletion,
};
use std::marker::PhantomData;
use std::pin::Pin;

/// Marker trait for sequence senders.
///
/// A sequence sender is a sender that can emit multiple items before completing.
/// Each item is itself a sender that the receiver processes via `set_next`.
pub trait SequenceSender: Sized {
    /// The types of item senders this sequence can emit.
    type ItemTypes: ItemTypes;

    /// Final completion signatures (typically just `set_value()`).
    type Completions: CompletionSignatures;
}

/// Trait for types that describe the item sender types a sequence can emit.
pub trait ItemTypes {
    /// Apply a function to each item type.
    fn for_each<F: ItemTypeVisitor>(&self, visitor: F);
}

/// Visitor for item types.
pub trait ItemTypeVisitor {
    /// Visit a specific item sender type.
    fn visit<S: Sender>(&mut self);
}

/// Empty item types - sequence emits no items.
pub struct EmptyItemTypes;

impl ItemTypes for EmptyItemTypes {
    fn for_each<F: ItemTypeVisitor>(&self, _visitor: F) {}
}

/// Single item type.
pub struct SingleItemType<S>(PhantomData<S>);

impl<S> Default for SingleItemType<S> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<S: Sender> ItemTypes for SingleItemType<S> {
    fn for_each<F: ItemTypeVisitor>(&self, mut visitor: F) {
        visitor.visit::<S>();
    }
}

/// The result of calling `set_next` on a sequence receiver.
///
/// This is a sender that completes with:
/// - `set_value()` - continue with next item
/// - `set_stopped()` - stop the sequence (break)
pub trait NextSender: Sender {}

/// A sequence receiver that can receive items via `set_next`.
pub trait SequenceReceiver: Receiver {
    /// The type returned by `set_next`.
    type NextSender: NextSender;

    /// Called for each item in the sequence.
    ///
    /// Returns a sender that, when completed:
    /// - `set_value()` means continue with the next item
    /// - `set_stopped()` means stop the sequence early
    fn set_next<S: Sender>(&mut self, item: S) -> Self::NextSender;
}

/// A sequence receiver that can receive specific item types.
pub trait SequenceReceiverOf<Items: ItemTypes>: SequenceReceiver {}

/// Trait for subscribing a receiver to a sequence sender.
///
/// This is analogous to `SenderTo` but for sequence senders.
pub trait SequenceSenderTo<R: SequenceReceiver>: SequenceSender {
    /// The operation state type.
    type Operation: OperationState;

    /// Subscribe the receiver to this sequence.
    fn subscribe(self, receiver: R) -> Self::Operation;
}

/// Result type for set_next operations.
pub enum SetNextResult {
    /// Continue with next item.
    Continue,
    /// Stop the sequence.
    Stop,
}

// ============================================================================
// Simple NextSender implementations
// ============================================================================

/// A next sender that always continues.
pub struct ContinueNextSender;

impl Sender for ContinueNextSender {
    type Completions = ValueCompletion<()>;
}

impl<R: ReceiverOf<()>> SenderTo<R> for ContinueNextSender {
    type Operation = ContinueNextOperation<R>;

    fn connect(self, receiver: R) -> Self::Operation {
        ContinueNextOperation { receiver }
    }
}

impl NextSender for ContinueNextSender {}

/// Operation for ContinueNextSender.
pub struct ContinueNextOperation<R> {
    receiver: R,
}

impl<R: ReceiverOf<()>> OperationState for ContinueNextOperation<R> {
    fn start(self: Pin<&mut Self>) {
        // Safety: We only move the receiver out, not the pinned self
        let this = unsafe { self.get_unchecked_mut() };
        let receiver = std::mem::replace(&mut this.receiver, unsafe { std::mem::zeroed() });
        receiver.set_value(());
    }
}

/// A next sender that always stops.
pub struct StopNextSender;

impl Sender for StopNextSender {
    type Completions = ValueCompletion<()>;
}

impl<R: ReceiverOfStopped> SenderTo<R> for StopNextSender {
    type Operation = StopNextOperation<R>;

    fn connect(self, receiver: R) -> Self::Operation {
        StopNextOperation { receiver }
    }
}

impl NextSender for StopNextSender {}

/// Operation for StopNextSender.
pub struct StopNextOperation<R> {
    receiver: R,
}

impl<R: ReceiverOfStopped> OperationState for StopNextOperation<R> {
    fn start(self: Pin<&mut Self>) {
        // Safety: We only move the receiver out
        let this = unsafe { self.get_unchecked_mut() };
        let receiver = std::mem::replace(&mut this.receiver, unsafe { std::mem::zeroed() });
        receiver.set_stopped();
    }
}

// ============================================================================
// JustNextSender - wraps a result to produce the appropriate completion
// ============================================================================

/// A next sender that completes based on a result.
pub struct JustNextSender {
    /// Whether to continue or stop.
    pub continue_: bool,
}

impl JustNextSender {
    /// Create a sender that continues.
    pub fn continue_() -> Self {
        Self { continue_: true }
    }

    /// Create a sender that stops.
    pub fn stop() -> Self {
        Self { continue_: false }
    }
}

impl Sender for JustNextSender {
    type Completions = ValueCompletion<()>;
}

impl<R: ReceiverOf<()> + ReceiverOfStopped> SenderTo<R> for JustNextSender {
    type Operation = JustNextOperation<R>;

    fn connect(self, receiver: R) -> Self::Operation {
        JustNextOperation {
            continue_: self.continue_,
            receiver,
        }
    }
}

impl NextSender for JustNextSender {}

/// Operation for JustNextSender.
pub struct JustNextOperation<R> {
    continue_: bool,
    receiver: R,
}

impl<R: ReceiverOf<()> + ReceiverOfStopped> OperationState for JustNextOperation<R> {
    fn start(self: Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };
        let receiver = std::mem::replace(&mut this.receiver, unsafe { std::mem::zeroed() });
        if this.continue_ {
            receiver.set_value(());
        } else {
            receiver.set_stopped();
        }
    }
}
