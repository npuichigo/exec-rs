//! Repeat adaptors - repeat sender execution.
//!
//! - `repeat_n(factory, n)` - repeat a sender N times
//! - `repeat_effect_until(factory)` - repeat until the sender returns true

use crate::traits::{
    CompletionSignatures, Environment, OperationState, Receiver, ReceiverOf, ReceiverOfError,
    ReceiverOfStopped, Sender, SenderTo,
};
use std::cell::UnsafeCell;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;

// ============================================================================
// repeat_n - repeat a sender N times
// ============================================================================

/// Repeat a sender factory N times.
///
/// Takes a factory function that produces senders. After N successful
/// completions, the overall sender completes with `()`.
///
/// # Example
///
/// ```rust
/// use exec_rs::adaptors::{repeat_n, then};
/// use exec_rs::factories::just;
/// use exec_rs::consumers::sync_wait;
/// use std::sync::atomic::{AtomicUsize, Ordering};
/// use std::sync::Arc;
///
/// let counter = Arc::new(AtomicUsize::new(0));
/// let counter_clone = counter.clone();
///
/// let factory = move || {
///     let counter = counter_clone.clone();
///     then(just(()), move |_| {
///         counter.fetch_add(1, Ordering::SeqCst);
///     })
/// };
///
/// let result = sync_wait(repeat_n(factory, 5)).unwrap();
/// assert_eq!(result, ());
/// assert_eq!(counter.load(Ordering::SeqCst), 5);
/// ```
pub fn repeat_n<F, S, E>(factory: F, count: usize) -> RepeatN<F, E>
where
    F: Fn() -> S,
    S: Sender<Completions: CompletionSignatures<Value = (), Error = E>>,
{
    RepeatN {
        factory,
        count,
        _phantom: PhantomData,
    }
}

/// A sender that repeats another sender N times.
#[derive(Clone)]
pub struct RepeatN<F, E> {
    factory: F,
    count: usize,
    _phantom: PhantomData<E>,
}

impl<F, E> std::fmt::Debug for RepeatN<F, E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RepeatN")
            .field("count", &self.count)
            .finish()
    }
}

/// Completion signatures for repeat_n.
pub struct RepeatNCompletions<E> {
    _phantom: PhantomData<E>,
}

impl<E> CompletionSignatures for RepeatNCompletions<E> {
    type Value = ();
    type Error = E;
    const SENDS_STOPPED: bool = true;
}

impl<F, S, E> Sender for RepeatN<F, E>
where
    F: Fn() -> S,
    S: Sender<Completions: CompletionSignatures<Value = (), Error = E>>,
{
    type Completions = RepeatNCompletions<E>;
}

impl<F, S, E, R> SenderTo<R> for RepeatN<F, E>
where
    F: Fn() -> S + Send + 'static,
    S: Sender<Completions: CompletionSignatures<Value = (), Error = E>>
        + SenderTo<RepeatNInnerReceiver<E, R>>
        + 'static,
    <S as SenderTo<RepeatNInnerReceiver<E, R>>>::Operation: Send,
    E: Send + 'static,
    R: ReceiverOf<()> + ReceiverOfError<E> + ReceiverOfStopped + Send + 'static,
{
    type Operation = RepeatNOperation<F, S, E, R>;

    fn connect(self, receiver: R) -> Self::Operation {
        RepeatNOperation {
            state: Arc::new(RepeatNState {
                factory: self.factory,
                remaining: UnsafeCell::new(self.count),
                receiver: UnsafeCell::new(Some(receiver)),
                current_op: UnsafeCell::new(None),
                _phantom: PhantomData,
            }),
        }
    }
}

struct RepeatNState<F, S, E, R>
where
    S: SenderTo<RepeatNInnerReceiver<E, R>>,
{
    factory: F,
    remaining: UnsafeCell<usize>,
    receiver: UnsafeCell<Option<R>>,
    current_op: UnsafeCell<Option<<S as SenderTo<RepeatNInnerReceiver<E, R>>>::Operation>>,
    _phantom: PhantomData<E>,
}

// SAFETY: Access is synchronized through the execution model
unsafe impl<F: Send, S, E, R: Send> Send for RepeatNState<F, S, E, R>
where
    S: SenderTo<RepeatNInnerReceiver<E, R>>,
    <S as SenderTo<RepeatNInnerReceiver<E, R>>>::Operation: Send,
{
}
unsafe impl<F: Send, S, E, R: Send> Sync for RepeatNState<F, S, E, R>
where
    S: SenderTo<RepeatNInnerReceiver<E, R>>,
    <S as SenderTo<RepeatNInnerReceiver<E, R>>>::Operation: Send,
{
}

/// Operation state for repeat_n.
pub struct RepeatNOperation<F, S, E, R>
where
    S: SenderTo<RepeatNInnerReceiver<E, R>>,
{
    state: Arc<RepeatNState<F, S, E, R>>,
}

/// Inner receiver for repeat iterations (breaks the recursive bound).
pub struct RepeatNInnerReceiver<E, R> {
    on_value: Box<dyn FnOnce() + Send>,
    on_error: Box<dyn FnOnce(E) + Send>,
    on_stopped: Box<dyn FnOnce() + Send>,
    _phantom: PhantomData<R>,
}

impl<E, R> Receiver for RepeatNInnerReceiver<E, R> {
    type Env = RepeatEnv;

    fn get_env(&self) -> &Self::Env {
        &RepeatEnv
    }
}

impl<E, R> ReceiverOf<()> for RepeatNInnerReceiver<E, R> {
    fn set_value(self, _: ()) {
        (self.on_value)();
    }
}

impl<E, R> ReceiverOfError<E> for RepeatNInnerReceiver<E, R> {
    fn set_error(self, error: E) {
        (self.on_error)(error);
    }
}

impl<E, R> ReceiverOfStopped for RepeatNInnerReceiver<E, R> {
    fn set_stopped(self) {
        (self.on_stopped)();
    }
}

/// Environment for repeat receivers.
pub struct RepeatEnv;

impl Environment for RepeatEnv {
    type StopToken = crate::stop::NeverStopToken;

    fn get_stop_token(&self) -> Self::StopToken {
        crate::stop::NeverStopToken
    }
}

impl<F, S, E, R> RepeatNState<F, S, E, R>
where
    F: Fn() -> S + Send + 'static,
    S: Sender<Completions: CompletionSignatures<Value = (), Error = E>>
        + SenderTo<RepeatNInnerReceiver<E, R>>
        + 'static,
    <S as SenderTo<RepeatNInnerReceiver<E, R>>>::Operation: Send,
    E: Send + 'static,
    R: ReceiverOf<()> + ReceiverOfError<E> + ReceiverOfStopped + Send + 'static,
{
    fn start_next(self: &Arc<Self>) {
        let remaining = unsafe { *self.remaining.get() };
        if remaining == 0 {
            // Done - complete with value
            let receiver = unsafe { (*self.receiver.get()).take() };
            if let Some(receiver) = receiver {
                receiver.set_value(());
            }
        } else {
            // Create callbacks that reference self
            let state_for_value = self.clone();
            let state_for_error = self.clone();
            let state_for_stopped = self.clone();

            let inner_receiver = RepeatNInnerReceiver {
                on_value: Box::new(move || state_for_value.on_value()),
                on_error: Box::new(move |e| state_for_error.on_error(e)),
                on_stopped: Box::new(move || state_for_stopped.on_stopped()),
                _phantom: PhantomData,
            };

            let sender = (self.factory)();
            let op = sender.connect(inner_receiver);
            unsafe {
                *self.current_op.get() = Some(op);
                if let Some(op) = (*self.current_op.get()).as_mut() {
                    Pin::new_unchecked(op).start();
                }
            }
        }
    }

    fn on_value(self: &Arc<Self>) {
        unsafe {
            let remaining = &mut *self.remaining.get();
            *remaining -= 1;
            *self.current_op.get() = None;
        }
        self.start_next();
    }

    fn on_error(self: &Arc<Self>, error: E) {
        let receiver = unsafe { (*self.receiver.get()).take() };
        if let Some(receiver) = receiver {
            receiver.set_error(error);
        }
    }

    fn on_stopped(self: &Arc<Self>) {
        let receiver = unsafe { (*self.receiver.get()).take() };
        if let Some(receiver) = receiver {
            receiver.set_stopped();
        }
    }
}

impl<F, S, E, R> OperationState for RepeatNOperation<F, S, E, R>
where
    F: Fn() -> S + Send + 'static,
    S: Sender<Completions: CompletionSignatures<Value = (), Error = E>>
        + SenderTo<RepeatNInnerReceiver<E, R>>
        + 'static,
    <S as SenderTo<RepeatNInnerReceiver<E, R>>>::Operation: Send,
    E: Send + 'static,
    R: ReceiverOf<()> + ReceiverOfError<E> + ReceiverOfStopped + Send + 'static,
{
    fn start(self: Pin<&mut Self>) {
        self.state.start_next();
    }
}

// ============================================================================
// repeat_effect_until - repeat until sender returns true
// ============================================================================

/// Repeat a sender until it returns `true`.
///
/// Takes a factory function that produces senders completing with `bool`.
/// When a sender returns `true`, the overall sender completes with `()`.
/// When it returns `false`, another sender is created and started.
///
/// # Example
///
/// ```rust
/// use exec_rs::adaptors::{repeat_effect_until, then};
/// use exec_rs::factories::just;
/// use exec_rs::consumers::sync_wait;
/// use std::sync::atomic::{AtomicUsize, Ordering};
/// use std::sync::Arc;
///
/// let counter = Arc::new(AtomicUsize::new(0));
/// let counter_clone = counter.clone();
///
/// let factory = move || {
///     let counter = counter_clone.clone();
///     then(just(()), move |_| {
///         let count = counter.fetch_add(1, Ordering::SeqCst) + 1;
///         count >= 5
///     })
/// };
///
/// let result = sync_wait(repeat_effect_until(factory)).unwrap();
/// assert_eq!(result, ());
/// assert_eq!(counter.load(Ordering::SeqCst), 5);
/// ```
pub fn repeat_effect_until<F, S, E>(factory: F) -> RepeatEffectUntil<F, E>
where
    F: Fn() -> S,
    S: Sender<Completions: CompletionSignatures<Value = bool, Error = E>>,
{
    RepeatEffectUntil {
        factory,
        _phantom: PhantomData,
    }
}

/// A sender that repeats another sender until it returns true.
#[derive(Clone)]
pub struct RepeatEffectUntil<F, E> {
    factory: F,
    _phantom: PhantomData<E>,
}

impl<F, E> std::fmt::Debug for RepeatEffectUntil<F, E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RepeatEffectUntil").finish()
    }
}

/// Completion signatures for repeat_effect_until.
pub struct RepeatEffectUntilCompletions<E> {
    _phantom: PhantomData<E>,
}

impl<E> CompletionSignatures for RepeatEffectUntilCompletions<E> {
    type Value = ();
    type Error = E;
    const SENDS_STOPPED: bool = true;
}

impl<F, S, E> Sender for RepeatEffectUntil<F, E>
where
    F: Fn() -> S,
    S: Sender<Completions: CompletionSignatures<Value = bool, Error = E>>,
{
    type Completions = RepeatEffectUntilCompletions<E>;
}

impl<F, S, E, R> SenderTo<R> for RepeatEffectUntil<F, E>
where
    F: Fn() -> S + Send + 'static,
    S: Sender<Completions: CompletionSignatures<Value = bool, Error = E>>
        + SenderTo<RepeatEffectUntilInnerReceiver<E, R>>
        + 'static,
    <S as SenderTo<RepeatEffectUntilInnerReceiver<E, R>>>::Operation: Send,
    E: Send + 'static,
    R: ReceiverOf<()> + ReceiverOfError<E> + ReceiverOfStopped + Send + 'static,
{
    type Operation = RepeatEffectUntilOperation<F, S, E, R>;

    fn connect(self, receiver: R) -> Self::Operation {
        RepeatEffectUntilOperation {
            state: Arc::new(RepeatEffectUntilState {
                factory: self.factory,
                receiver: UnsafeCell::new(Some(receiver)),
                current_op: UnsafeCell::new(None),
                _phantom: PhantomData,
            }),
        }
    }
}

struct RepeatEffectUntilState<F, S, E, R>
where
    S: SenderTo<RepeatEffectUntilInnerReceiver<E, R>>,
{
    factory: F,
    receiver: UnsafeCell<Option<R>>,
    current_op:
        UnsafeCell<Option<<S as SenderTo<RepeatEffectUntilInnerReceiver<E, R>>>::Operation>>,
    _phantom: PhantomData<E>,
}

// SAFETY: Access is synchronized through the execution model
unsafe impl<F: Send, S, E, R: Send> Send for RepeatEffectUntilState<F, S, E, R>
where
    S: SenderTo<RepeatEffectUntilInnerReceiver<E, R>>,
    <S as SenderTo<RepeatEffectUntilInnerReceiver<E, R>>>::Operation: Send,
{
}
unsafe impl<F: Send, S, E, R: Send> Sync for RepeatEffectUntilState<F, S, E, R>
where
    S: SenderTo<RepeatEffectUntilInnerReceiver<E, R>>,
    <S as SenderTo<RepeatEffectUntilInnerReceiver<E, R>>>::Operation: Send,
{
}

/// Operation state for repeat_effect_until.
pub struct RepeatEffectUntilOperation<F, S, E, R>
where
    S: SenderTo<RepeatEffectUntilInnerReceiver<E, R>>,
{
    state: Arc<RepeatEffectUntilState<F, S, E, R>>,
}

/// Inner receiver for repeat_effect_until iterations.
pub struct RepeatEffectUntilInnerReceiver<E, R> {
    on_value: Box<dyn FnOnce(bool) + Send>,
    on_error: Box<dyn FnOnce(E) + Send>,
    on_stopped: Box<dyn FnOnce() + Send>,
    _phantom: PhantomData<R>,
}

impl<E, R> Receiver for RepeatEffectUntilInnerReceiver<E, R> {
    type Env = RepeatEnv;

    fn get_env(&self) -> &Self::Env {
        &RepeatEnv
    }
}

impl<E, R> ReceiverOf<bool> for RepeatEffectUntilInnerReceiver<E, R> {
    fn set_value(self, done: bool) {
        (self.on_value)(done);
    }
}

impl<E, R> ReceiverOfError<E> for RepeatEffectUntilInnerReceiver<E, R> {
    fn set_error(self, error: E) {
        (self.on_error)(error);
    }
}

impl<E, R> ReceiverOfStopped for RepeatEffectUntilInnerReceiver<E, R> {
    fn set_stopped(self) {
        (self.on_stopped)();
    }
}

impl<F, S, E, R> RepeatEffectUntilState<F, S, E, R>
where
    F: Fn() -> S + Send + 'static,
    S: Sender<Completions: CompletionSignatures<Value = bool, Error = E>>
        + SenderTo<RepeatEffectUntilInnerReceiver<E, R>>
        + 'static,
    <S as SenderTo<RepeatEffectUntilInnerReceiver<E, R>>>::Operation: Send,
    E: Send + 'static,
    R: ReceiverOf<()> + ReceiverOfError<E> + ReceiverOfStopped + Send + 'static,
{
    fn start_iteration(self: &Arc<Self>) {
        let state_for_value = self.clone();
        let state_for_error = self.clone();
        let state_for_stopped = self.clone();

        let inner_receiver = RepeatEffectUntilInnerReceiver {
            on_value: Box::new(move |done| state_for_value.on_value(done)),
            on_error: Box::new(move |e| state_for_error.on_error(e)),
            on_stopped: Box::new(move || state_for_stopped.on_stopped()),
            _phantom: PhantomData,
        };

        let sender = (self.factory)();
        let op = sender.connect(inner_receiver);
        unsafe {
            *self.current_op.get() = Some(op);
            if let Some(op) = (*self.current_op.get()).as_mut() {
                Pin::new_unchecked(op).start();
            }
        }
    }

    fn on_value(self: &Arc<Self>, done: bool) {
        if done {
            let receiver = unsafe { (*self.receiver.get()).take() };
            if let Some(receiver) = receiver {
                receiver.set_value(());
            }
        } else {
            unsafe {
                *self.current_op.get() = None;
            }
            self.start_iteration();
        }
    }

    fn on_error(self: &Arc<Self>, error: E) {
        let receiver = unsafe { (*self.receiver.get()).take() };
        if let Some(receiver) = receiver {
            receiver.set_error(error);
        }
    }

    fn on_stopped(self: &Arc<Self>) {
        let receiver = unsafe { (*self.receiver.get()).take() };
        if let Some(receiver) = receiver {
            receiver.set_stopped();
        }
    }
}

impl<F, S, E, R> OperationState for RepeatEffectUntilOperation<F, S, E, R>
where
    F: Fn() -> S + Send + 'static,
    S: Sender<Completions: CompletionSignatures<Value = bool, Error = E>>
        + SenderTo<RepeatEffectUntilInnerReceiver<E, R>>
        + 'static,
    <S as SenderTo<RepeatEffectUntilInnerReceiver<E, R>>>::Operation: Send,
    E: Send + 'static,
    R: ReceiverOf<()> + ReceiverOfError<E> + ReceiverOfStopped + Send + 'static,
{
    fn start(self: Pin<&mut Self>) {
        self.state.start_iteration();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adaptors::then;
    use crate::consumers::sync_wait;
    use crate::factories::just;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn test_repeat_n_zero() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let factory = move || {
            let counter = counter_clone.clone();
            then(just(()), move |_| {
                counter.fetch_add(1, Ordering::SeqCst);
            })
        };

        let result = sync_wait(repeat_n(factory, 0)).unwrap();
        assert_eq!(result, ());
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_repeat_n_once() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let factory = move || {
            let counter = counter_clone.clone();
            then(just(()), move |_| {
                counter.fetch_add(1, Ordering::SeqCst);
            })
        };

        let result = sync_wait(repeat_n(factory, 1)).unwrap();
        assert_eq!(result, ());
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_repeat_n_multiple() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let factory = move || {
            let counter = counter_clone.clone();
            then(just(()), move |_| {
                counter.fetch_add(1, Ordering::SeqCst);
            })
        };

        let result = sync_wait(repeat_n(factory, 5)).unwrap();
        assert_eq!(result, ());
        assert_eq!(counter.load(Ordering::SeqCst), 5);
    }

    #[test]
    fn test_repeat_effect_until_immediate() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let factory = move || {
            let counter = counter_clone.clone();
            then(just(()), move |_| {
                counter.fetch_add(1, Ordering::SeqCst);
                true
            })
        };

        let result = sync_wait(repeat_effect_until(factory)).unwrap();
        assert_eq!(result, ());
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_repeat_effect_until_multiple() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let factory = move || {
            let counter = counter_clone.clone();
            then(just(()), move |_| {
                let count = counter.fetch_add(1, Ordering::SeqCst) + 1;
                count >= 5
            })
        };

        let result = sync_wait(repeat_effect_until(factory)).unwrap();
        assert_eq!(result, ());
        assert_eq!(counter.load(Ordering::SeqCst), 5);
    }
}
