//! Just sender factories.
//!
//! These factories create senders that complete immediately with a value,
//! error, or stopped signal.

use crate::traits::{
    CompletionSignatures, JustCompletions, JustErrorCompletions, OperationState, ReceiverOf,
    ReceiverOfError, ReceiverOfStopped, Sender, SenderTo, StoppedCompletion,
};
use std::marker::PhantomData;
use std::pin::Pin;

/// Create a sender that completes with a value.
///
/// The sender completes immediately when started, calling `set_value` on
/// the receiver with the provided value.
///
/// # Example
///
/// ```rust
/// use exec_rs::factories::just;
/// use exec_rs::consumers::sync_wait;
///
/// let result = sync_wait(just(42)).unwrap();
/// assert_eq!(result, 42);
/// ```
pub fn just<T>(value: T) -> Just<T> {
    Just { value }
}

/// Create a sender that completes with an error.
///
/// The sender completes immediately when started, calling `set_error` on
/// the receiver with the provided error.
///
/// # Example
///
/// ```rust,ignore
/// use exec_rs::factories::just_error;
/// use exec_rs::consumers::sync_wait;
///
/// let result: Result<(), _> = sync_wait(just_error("oops"));
/// assert!(result.is_err());
/// ```
pub fn just_error<E>(error: E) -> JustError<E> {
    JustError { error }
}

/// Create a sender that completes with stopped.
///
/// The sender completes immediately when started, calling `set_stopped` on
/// the receiver.
///
/// # Example
///
/// ```rust,ignore
/// use exec_rs::factories::just_stopped;
/// use exec_rs::consumers::sync_wait;
///
/// let result = sync_wait(just_stopped());
/// assert!(matches!(result, Err(SyncWaitError::Stopped)));
/// ```
pub fn just_stopped() -> JustStopped {
    JustStopped
}

/// Create a sender that calls a function to produce a value.
///
/// The function is called when the sender is started, and its result
/// is passed to `set_value`.
///
/// # Example
///
/// ```rust,ignore
/// use exec_rs::factories::just_from;
/// use exec_rs::consumers::sync_wait;
///
/// let result = sync_wait(just_from(|| 42)).unwrap();
/// assert_eq!(result, 42);
/// ```
pub fn just_from<F, T>(f: F) -> JustFrom<F>
where
    F: FnOnce() -> T,
{
    JustFrom { f }
}

/// Create a sender that calls a function to produce an error.
pub fn just_error_from<F, E>(f: F) -> JustErrorFrom<F>
where
    F: FnOnce() -> E,
{
    JustErrorFrom { f }
}

/// Create a sender that calls a function and then completes with stopped.
pub fn just_stopped_from<F>(f: F) -> JustStoppedFrom<F>
where
    F: FnOnce(),
{
    JustStoppedFrom { f }
}

// ============================================================================
// Just<T> - Sender that completes with a value
// ============================================================================

/// A sender that completes immediately with a value.
#[derive(Debug, Clone)]
pub struct Just<T> {
    value: T,
}

impl<T> Sender for Just<T> {
    type Completions = JustCompletions<T>;
}

impl<T, R> SenderTo<R> for Just<T>
where
    R: ReceiverOf<T>,
{
    type Operation = JustOperation<T, R>;

    fn connect(self, receiver: R) -> Self::Operation {
        JustOperation {
            value: Some(self.value),
            receiver: Some(receiver),
        }
    }
}

/// Operation state for Just sender.
pub struct JustOperation<T, R> {
    value: Option<T>,
    receiver: Option<R>,
}

impl<T, R: ReceiverOf<T>> OperationState for JustOperation<T, R> {
    fn start(self: Pin<&mut Self>) {
        // SAFETY: We only access Option fields which don't need pinning
        let this = unsafe { self.get_unchecked_mut() };
        if let (Some(value), Some(receiver)) = (this.value.take(), this.receiver.take()) {
            receiver.set_value(value);
        }
    }
}

// ============================================================================
// JustError<E> - Sender that completes with an error
// ============================================================================

/// A sender that completes immediately with an error.
#[derive(Debug, Clone)]
pub struct JustError<E> {
    error: E,
}

impl<E> Sender for JustError<E> {
    type Completions = JustErrorCompletions<E>;
}

impl<E, R> SenderTo<R> for JustError<E>
where
    R: ReceiverOfError<E>,
{
    type Operation = JustErrorOperation<E, R>;

    fn connect(self, receiver: R) -> Self::Operation {
        JustErrorOperation {
            error: Some(self.error),
            receiver: Some(receiver),
        }
    }
}

/// Operation state for JustError sender.
pub struct JustErrorOperation<E, R> {
    error: Option<E>,
    receiver: Option<R>,
}

impl<E, R: ReceiverOfError<E>> OperationState for JustErrorOperation<E, R> {
    fn start(self: Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };
        if let (Some(error), Some(receiver)) = (this.error.take(), this.receiver.take()) {
            receiver.set_error(error);
        }
    }
}

// ============================================================================
// JustStopped - Sender that completes with stopped
// ============================================================================

/// A sender that completes immediately with stopped.
#[derive(Debug, Clone, Copy, Default)]
pub struct JustStopped;

impl Sender for JustStopped {
    type Completions = StoppedCompletion;
}

impl<R> SenderTo<R> for JustStopped
where
    R: ReceiverOfStopped,
{
    type Operation = JustStoppedOperation<R>;

    fn connect(self, receiver: R) -> Self::Operation {
        JustStoppedOperation {
            receiver: Some(receiver),
        }
    }
}

/// Operation state for JustStopped sender.
pub struct JustStoppedOperation<R> {
    receiver: Option<R>,
}

impl<R: ReceiverOfStopped> OperationState for JustStoppedOperation<R> {
    fn start(self: Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };
        if let Some(receiver) = this.receiver.take() {
            receiver.set_stopped();
        }
    }
}

// ============================================================================
// JustFrom<F> - Sender that calls a function to produce a value
// ============================================================================

/// A sender that calls a function to produce a value.
#[derive(Debug, Clone)]
pub struct JustFrom<F> {
    f: F,
}

/// Completions type for JustFrom that extracts the return type of F.
pub struct JustFromCompletions<T>(PhantomData<T>);

impl<T> CompletionSignatures for JustFromCompletions<T> {
    type Value = T;
    type Error = std::convert::Infallible;
    const SENDS_STOPPED: bool = false;
}

impl<F, T> Sender for JustFrom<F>
where
    F: FnOnce() -> T,
{
    type Completions = JustFromCompletions<T>;
}

impl<F, T, R> SenderTo<R> for JustFrom<F>
where
    F: FnOnce() -> T,
    R: ReceiverOf<T>,
{
    type Operation = JustFromOperation<F, R>;

    fn connect(self, receiver: R) -> Self::Operation {
        JustFromOperation {
            f: Some(self.f),
            receiver: Some(receiver),
        }
    }
}

/// Operation state for JustFrom sender.
pub struct JustFromOperation<F, R> {
    f: Option<F>,
    receiver: Option<R>,
}

impl<F, T, R> OperationState for JustFromOperation<F, R>
where
    F: FnOnce() -> T,
    R: ReceiverOf<T>,
{
    fn start(self: Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };
        if let (Some(f), Some(receiver)) = (this.f.take(), this.receiver.take()) {
            let value = f();
            receiver.set_value(value);
        }
    }
}

// ============================================================================
// JustErrorFrom<F> - Sender that calls a function to produce an error
// ============================================================================

/// A sender that calls a function to produce an error.
#[derive(Debug, Clone)]
pub struct JustErrorFrom<F> {
    f: F,
}

/// Completions type for JustErrorFrom.
pub struct JustErrorFromCompletions<E>(PhantomData<E>);

impl<E> CompletionSignatures for JustErrorFromCompletions<E> {
    type Value = ();
    type Error = E;
    const SENDS_STOPPED: bool = false;
}

impl<F, E> Sender for JustErrorFrom<F>
where
    F: FnOnce() -> E,
{
    type Completions = JustErrorFromCompletions<E>;
}

impl<F, E, R> SenderTo<R> for JustErrorFrom<F>
where
    F: FnOnce() -> E,
    R: ReceiverOfError<E>,
{
    type Operation = JustErrorFromOperation<F, R>;

    fn connect(self, receiver: R) -> Self::Operation {
        JustErrorFromOperation {
            f: Some(self.f),
            receiver: Some(receiver),
        }
    }
}

/// Operation state for JustErrorFrom sender.
pub struct JustErrorFromOperation<F, R> {
    f: Option<F>,
    receiver: Option<R>,
}

impl<F, E, R> OperationState for JustErrorFromOperation<F, R>
where
    F: FnOnce() -> E,
    R: ReceiverOfError<E>,
{
    fn start(self: Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };
        if let (Some(f), Some(receiver)) = (this.f.take(), this.receiver.take()) {
            let error = f();
            receiver.set_error(error);
        }
    }
}

// ============================================================================
// JustStoppedFrom<F> - Sender that calls a function then completes with stopped
// ============================================================================

/// A sender that calls a function then completes with stopped.
#[derive(Debug, Clone)]
pub struct JustStoppedFrom<F> {
    f: F,
}

impl<F> Sender for JustStoppedFrom<F>
where
    F: FnOnce(),
{
    type Completions = StoppedCompletion;
}

impl<F, R> SenderTo<R> for JustStoppedFrom<F>
where
    F: FnOnce(),
    R: ReceiverOfStopped,
{
    type Operation = JustStoppedFromOperation<F, R>;

    fn connect(self, receiver: R) -> Self::Operation {
        JustStoppedFromOperation {
            f: Some(self.f),
            receiver: Some(receiver),
        }
    }
}

/// Operation state for JustStoppedFrom sender.
pub struct JustStoppedFromOperation<F, R> {
    f: Option<F>,
    receiver: Option<R>,
}

impl<F, R> OperationState for JustStoppedFromOperation<F, R>
where
    F: FnOnce(),
    R: ReceiverOfStopped,
{
    fn start(self: Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };
        if let (Some(f), Some(receiver)) = (this.f.take(), this.receiver.take()) {
            f();
            receiver.set_stopped();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consumers::sync_wait;
    use crate::traits::{Sender, VoidReceiver};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;

    // ========================================================================
    // test_just.cpp equivalents
    // ========================================================================

    #[test]
    fn test_just_simple_value() {
        let result = sync_wait(just(42)).unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn test_just_string_value() {
        let result = sync_wait(just(String::from("hello"))).unwrap();
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_just_unit_value() {
        let result = sync_wait(just(())).unwrap();
        assert_eq!(result, ());
    }

    #[test]
    fn test_just_tuple_value() {
        let result = sync_wait(just((1, 2, 3))).unwrap();
        assert_eq!(result, (1, 2, 3));
    }

    #[test]
    fn test_just_is_sender() {
        fn assert_sender<S: Sender>(_: &S) {}
        let s = just(42);
        assert_sender(&s);
    }

    #[test]
    fn test_just_move_only_value() {
        let boxed = Box::new(42);
        let result = sync_wait(just(boxed)).unwrap();
        assert_eq!(*result, 42);
    }

    #[test]
    fn test_just_zero_copy() {
        #[derive(Debug)]
        struct CountedValue {
            value: i32,
            clone_count: Arc<AtomicUsize>,
        }

        impl Clone for CountedValue {
            fn clone(&self) -> Self {
                self.clone_count.fetch_add(1, Ordering::SeqCst);
                CountedValue {
                    value: self.value,
                    clone_count: self.clone_count.clone(),
                }
            }
        }

        let clone_count = Arc::new(AtomicUsize::new(0));
        let v = CountedValue {
            value: 42,
            clone_count: clone_count.clone(),
        };

        let result = sync_wait(just(v)).unwrap();
        assert_eq!(result.value, 42);
        assert_eq!(clone_count.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_just_with_vec() {
        let v = vec![1, 2, 3, 4, 5];
        let result = sync_wait(just(v)).unwrap();
        assert_eq!(result, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_just_with_option() {
        let result = sync_wait(just(Some(42))).unwrap();
        assert_eq!(result, Some(42));
    }

    #[test]
    fn test_just_with_result() {
        let result = sync_wait(just(Ok::<i32, &str>(42))).unwrap();
        assert_eq!(result, Ok(42));
    }

    // ========================================================================
    // test_just_error.cpp equivalents
    // ========================================================================

    #[test]
    fn test_just_error_string() {
        let result = sync_wait(just_error::<_>("error"));
        assert!(result.is_err());
    }

    #[test]
    fn test_just_error_custom_type() {
        #[derive(Debug, PartialEq)]
        struct MyError(i32);

        let result = sync_wait(just_error::<_>(MyError(42)));
        match result {
            Err(crate::consumers::SyncWaitError::Error(e)) => assert_eq!(e, MyError(42)),
            _ => panic!("Expected error"),
        }
    }

    #[test]
    fn test_just_error_is_sender() {
        fn assert_sender<S: Sender>(_: &S) {}
        let s = just_error::<_>("error");
        assert_sender(&s);
    }

    // ========================================================================
    // test_just_stopped.cpp equivalents
    // ========================================================================

    #[test]
    fn test_just_stopped_completes_with_stopped() {
        let result = sync_wait(just_stopped());
        match result {
            Err(crate::consumers::SyncWaitError::Stopped) => (),
            _ => panic!("Expected stopped"),
        }
    }

    #[test]
    fn test_just_stopped_is_sender() {
        fn assert_sender<S: Sender>(_: &S) {}
        let s = just_stopped();
        assert_sender(&s);
    }

    // ========================================================================
    // test_just_from equivalents
    // ========================================================================

    #[test]
    fn test_just_from_simple() {
        let result = sync_wait(just_from(|| 42)).unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn test_just_from_with_capture() {
        let x = 10;
        let result = sync_wait(just_from(move || x * 2)).unwrap();
        assert_eq!(result, 20);
    }

    #[test]
    fn test_just_from_string() {
        let result = sync_wait(just_from(|| String::from("computed"))).unwrap();
        assert_eq!(result, "computed");
    }

    #[test]
    fn test_just_from_side_effect() {
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        let result = sync_wait(just_from(move || {
            called_clone.store(true, Ordering::SeqCst);
            42
        }))
        .unwrap();

        assert_eq!(result, 42);
        assert!(called.load(Ordering::SeqCst));
    }

    #[test]
    fn test_just_from_closure_not_called_until_started() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let sender = just_from(move || {
            counter_clone.fetch_add(1, Ordering::SeqCst);
            "done"
        });

        // Factory function should not be called until started
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        let result = sync_wait(sender).unwrap();
        assert_eq!(result, "done");
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    // ========================================================================
    // Original basic tests
    // ========================================================================

    #[test]
    fn test_just_creates_sender() {
        let sender = just(42);
        let receiver = VoidReceiver::new(());
        let mut op = sender.connect(receiver);
        Pin::new(&mut op).start();
    }

    #[test]
    fn test_just_error_creates_sender() {
        let sender = just_error("error");
        let receiver = VoidReceiver::new(());
        let mut op = sender.connect(receiver);
        Pin::new(&mut op).start();
    }

    #[test]
    fn test_just_stopped_creates_sender() {
        let sender = just_stopped();
        let receiver = VoidReceiver::new(());
        let mut op = sender.connect(receiver);
        Pin::new(&mut op).start();
    }

    #[test]
    fn test_just_from_calls_function() {
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        let sender = just_from(move || {
            called_clone.store(true, Ordering::SeqCst);
            42
        });

        let receiver = VoidReceiver::new(());
        let mut op = sender.connect(receiver);

        assert!(!called.load(Ordering::SeqCst));
        Pin::new(&mut op).start();
        assert!(called.load(Ordering::SeqCst));
    }
}
