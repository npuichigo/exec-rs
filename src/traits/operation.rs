//! Operation state trait for connected sender-receiver pairs.
//!
//! When a sender is connected to a receiver, the result is an operation state.
//! The operation state owns both the sender's work and the receiver, and
//! provides a `start` method to begin execution.

use std::pin::Pin;

/// The operation state trait.
///
/// An operation state represents a connected sender-receiver pair that is
/// ready to execute. Once `start` is called, the operation will eventually
/// complete by calling one of the receiver's completion methods.
///
/// # Pin Requirement
///
/// Operation states must be pinned before `start` is called because:
/// 1. The operation may contain self-referential data
/// 2. The receiver captures a pointer to the operation state
/// 3. Moving the operation after start would invalidate these pointers
///
/// # Example
///
/// ```rust,ignore
/// let mut op = sender.connect(receiver);
/// let pinned = Pin::new(&mut op);
/// pinned.start();
/// // op will complete asynchronously
/// ```
pub trait OperationState {
    /// Start the operation.
    ///
    /// This begins execution of the connected sender. The operation will
    /// eventually complete by calling one of the receiver's completion
    /// methods (`set_value`, `set_error`, or `set_stopped`).
    ///
    /// # Panics
    ///
    /// Implementations should not panic. If an error occurs during start,
    /// it should be propagated via `set_error`.
    ///
    /// # Safety
    ///
    /// This method requires `Pin<&mut Self>` because the operation state
    /// must not be moved after start is called. The implementation may
    /// store self-referential pointers that would be invalidated by a move.
    fn start(self: Pin<&mut Self>);
}

/// Extension trait for operation states that can be started immediately.
pub trait OperationStateExt: OperationState + Sized {
    /// Start the operation by pinning it in place.
    ///
    /// This is a convenience method that pins the operation and starts it.
    /// The caller must ensure the operation is not moved after this call.
    fn start_pinned(&mut self)
    where
        Self: Unpin,
    {
        Pin::new(self).start();
    }
}

impl<O: OperationState> OperationStateExt for O {}

/// A no-op operation state that does nothing when started.
///
/// This is useful for testing or as a placeholder.
#[derive(Debug, Default)]
pub struct NoopOperation;

impl OperationState for NoopOperation {
    fn start(self: Pin<&mut Self>) {
        // Do nothing
    }
}

/// An operation state that immediately calls set_value on the receiver.
pub struct ImmediateValueOperation<R, V> {
    receiver: Option<R>,
    value: Option<V>,
}

impl<R, V> ImmediateValueOperation<R, V> {
    /// Create a new immediate value operation.
    pub fn new(receiver: R, value: V) -> Self {
        Self {
            receiver: Some(receiver),
            value: Some(value),
        }
    }
}

impl<R, V> OperationState for ImmediateValueOperation<R, V>
where
    R: crate::traits::ReceiverOf<V>,
{
    fn start(self: Pin<&mut Self>) {
        // SAFETY: We only access receiver and value, which are not pinned
        let this = unsafe { self.get_unchecked_mut() };
        if let (Some(receiver), Some(value)) = (this.receiver.take(), this.value.take()) {
            receiver.set_value(value);
        }
    }
}

/// An operation state that immediately calls set_error on the receiver.
pub struct ImmediateErrorOperation<R, E> {
    receiver: Option<R>,
    error: Option<E>,
}

impl<R, E> ImmediateErrorOperation<R, E> {
    /// Create a new immediate error operation.
    pub fn new(receiver: R, error: E) -> Self {
        Self {
            receiver: Some(receiver),
            error: Some(error),
        }
    }
}

impl<R, E> OperationState for ImmediateErrorOperation<R, E>
where
    R: crate::traits::ReceiverOfError<E>,
{
    fn start(self: Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };
        if let (Some(receiver), Some(error)) = (this.receiver.take(), this.error.take()) {
            receiver.set_error(error);
        }
    }
}

/// An operation state that immediately calls set_stopped on the receiver.
pub struct ImmediateStoppedOperation<R> {
    receiver: Option<R>,
}

impl<R> ImmediateStoppedOperation<R> {
    /// Create a new immediate stopped operation.
    pub fn new(receiver: R) -> Self {
        Self {
            receiver: Some(receiver),
        }
    }
}

impl<R> OperationState for ImmediateStoppedOperation<R>
where
    R: crate::traits::ReceiverOfStopped,
{
    fn start(self: Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };
        if let Some(receiver) = this.receiver.take() {
            receiver.set_stopped();
        }
    }
}
