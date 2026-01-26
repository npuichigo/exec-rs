//! Repeat adaptors - repeat sender execution.
//!
//! - `repeat_effect(factory)` - repeat a sender forever (until error/stopped)
//! - `repeat_n(factory, n)` - repeat a sender N times
//! - `repeat_effect_until(factory)` - repeat until the sender returns true
//!
//! These adaptors use the trampoline scheduler internally to prevent
//! stack overflow when iterating many times.

use crate::schedulers::trampoline_submit;
use crate::traits::{
    CompletionSignatures, Environment, OperationState, Receiver, ReceiverOf, ReceiverOfError,
    ReceiverOfStopped, Sender, SenderTo,
};
use std::cell::UnsafeCell;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;

// ============================================================================
// repeat_effect - repeat a sender forever
// ============================================================================

/// Repeat a sender forever (until error or stopped).
///
/// This is useful for accept loops that should run continuously.
/// The sender must implement `Clone` and complete with `()`. After each
/// completion, the sender is cloned and started again.
///
/// This matches the stdexec pattern where the sender is stored and
/// reconnected each iteration.
///
/// # Example
///
/// ```rust
/// use exec_rs::adaptors::{repeat_effect, then};
/// use exec_rs::factories::just;
/// use std::sync::atomic::{AtomicUsize, Ordering};
/// use std::sync::Arc;
///
/// let counter = Arc::new(AtomicUsize::new(0));
/// let counter_clone = counter.clone();
///
/// // This would run forever, so we use repeat_n for the test
/// let sender = then(just(()), move |_: ()| {
///     counter_clone.fetch_add(1, Ordering::SeqCst);
/// });
///
/// // repeat_effect(sender) would run forever
/// ```
pub fn repeat_effect<S, E>(sender: S) -> RepeatEffect<S, E>
where
    S: Sender<Completions: CompletionSignatures<Value = (), Error = E>> + Clone,
{
    RepeatEffect {
        sender,
        _phantom: PhantomData,
    }
}

/// A sender that repeats another sender forever.
pub struct RepeatEffect<S, E> {
    sender: S,
    _phantom: PhantomData<E>,
}

impl<S: Clone, E> Clone for RepeatEffect<S, E> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<S, E> std::fmt::Debug for RepeatEffect<S, E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RepeatEffect").finish()
    }
}

/// Completion signatures for repeat_effect.
/// Note: Value is `!` (never) since it runs forever, but we use () for simplicity.
pub struct RepeatEffectCompletions<E> {
    _phantom: PhantomData<E>,
}

impl<E> CompletionSignatures for RepeatEffectCompletions<E> {
    type Value = (); // Never actually completes with value
    type Error = E;
    const SENDS_STOPPED: bool = true;
}

impl<S, E> Sender for RepeatEffect<S, E>
where
    S: Sender<Completions: CompletionSignatures<Value = (), Error = E>> + Clone,
{
    type Completions = RepeatEffectCompletions<E>;
}

impl<S, E, R> SenderTo<R> for RepeatEffect<S, E>
where
    S: Sender<Completions: CompletionSignatures<Value = (), Error = E>>
        + SenderTo<RepeatEffectInnerReceiver<E, R>>
        + Clone
        + Send
        + 'static,
    <S as SenderTo<RepeatEffectInnerReceiver<E, R>>>::Operation: Send,
    E: Send + 'static,
    R: ReceiverOf<()> + ReceiverOfError<E> + ReceiverOfStopped + Send + 'static,
{
    type Operation = RepeatEffectOperation<S, E, R>;

    fn connect(self, receiver: R) -> Self::Operation {
        RepeatEffectOperation {
            state: Arc::new(RepeatEffectState {
                sender: self.sender,
                receiver: UnsafeCell::new(Some(receiver)),
                current_op: UnsafeCell::new(None),
                _phantom: PhantomData,
            }),
        }
    }
}

struct RepeatEffectState<S, E, R>
where
    S: SenderTo<RepeatEffectInnerReceiver<E, R>>,
{
    sender: S,
    receiver: UnsafeCell<Option<R>>,
    current_op: UnsafeCell<Option<<S as SenderTo<RepeatEffectInnerReceiver<E, R>>>::Operation>>,
    _phantom: PhantomData<E>,
}

unsafe impl<S: Send, E, R: Send> Send for RepeatEffectState<S, E, R>
where
    S: SenderTo<RepeatEffectInnerReceiver<E, R>>,
    <S as SenderTo<RepeatEffectInnerReceiver<E, R>>>::Operation: Send,
{
}
unsafe impl<S: Send, E, R: Send> Sync for RepeatEffectState<S, E, R>
where
    S: SenderTo<RepeatEffectInnerReceiver<E, R>>,
    <S as SenderTo<RepeatEffectInnerReceiver<E, R>>>::Operation: Send,
{
}

/// Operation state for repeat_effect.
pub struct RepeatEffectOperation<S, E, R>
where
    S: SenderTo<RepeatEffectInnerReceiver<E, R>>,
{
    state: Arc<RepeatEffectState<S, E, R>>,
}

/// Inner receiver for repeat_effect iterations.
pub struct RepeatEffectInnerReceiver<E, R> {
    on_value: Box<dyn FnOnce(()) + Send>,
    on_error: Box<dyn FnOnce(E) + Send>,
    on_stopped: Box<dyn FnOnce() + Send>,
    _phantom: PhantomData<R>,
}

impl<E, R> Receiver for RepeatEffectInnerReceiver<E, R> {
    type Env = RepeatEnv;

    fn get_env(&self) -> &Self::Env {
        &RepeatEnv
    }
}

impl<E, R> ReceiverOf<()> for RepeatEffectInnerReceiver<E, R> {
    fn set_value(self, _: ()) {
        (self.on_value)(());
    }
}

impl<E, R> ReceiverOfError<E> for RepeatEffectInnerReceiver<E, R> {
    fn set_error(self, error: E) {
        (self.on_error)(error);
    }
}

impl<E, R> ReceiverOfStopped for RepeatEffectInnerReceiver<E, R> {
    fn set_stopped(self) {
        (self.on_stopped)();
    }
}

impl<S, E, R> RepeatEffectState<S, E, R>
where
    S: Sender<Completions: CompletionSignatures<Value = (), Error = E>>
        + SenderTo<RepeatEffectInnerReceiver<E, R>>
        + Clone
        + Send
        + 'static,
    <S as SenderTo<RepeatEffectInnerReceiver<E, R>>>::Operation: Send,
    E: Send + 'static,
    R: ReceiverOf<()> + ReceiverOfError<E> + ReceiverOfStopped + Send + 'static,
{
    fn start_iteration(self: &Arc<Self>) {
        let state_for_value = self.clone();
        let state_for_error = self.clone();
        let state_for_stopped = self.clone();

        let inner_receiver = RepeatEffectInnerReceiver {
            on_value: Box::new(move |_| state_for_value.on_value()),
            on_error: Box::new(move |e| state_for_error.on_error(e)),
            on_stopped: Box::new(move || state_for_stopped.on_stopped()),
            _phantom: PhantomData,
        };

        // Clone the sender for this iteration (like stdexec reconnecting)
        let sender = self.sender.clone();
        let op = sender.connect(inner_receiver);
        unsafe {
            *self.current_op.get() = Some(op);
            if let Some(op) = (*self.current_op.get()).as_mut() {
                Pin::new_unchecked(op).start();
            }
        }
    }

    fn on_value(self: &Arc<Self>) {
        // Clear current op and start next iteration
        unsafe {
            *self.current_op.get() = None;
        }
        // Use trampoline to prevent stack overflow
        let state = self.clone();
        trampoline_submit(move || {
            state.start_iteration();
        });
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

impl<S, E, R> OperationState for RepeatEffectOperation<S, E, R>
where
    S: Sender<Completions: CompletionSignatures<Value = (), Error = E>>
        + SenderTo<RepeatEffectInnerReceiver<E, R>>
        + Clone
        + Send
        + 'static,
    <S as SenderTo<RepeatEffectInnerReceiver<E, R>>>::Operation: Send,
    E: Send + 'static,
    R: ReceiverOf<()> + ReceiverOfError<E> + ReceiverOfStopped + Send + 'static,
{
    fn start(self: Pin<&mut Self>) {
        self.state.start_iteration();
    }
}

// ============================================================================
// repeat_n - repeat a sender N times
// ============================================================================

/// Repeat a sender N times.
///
/// The sender must implement `Clone` and complete with `()`. After N successful
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
/// let sender = then(just(()), move |_: ()| {
///     counter_clone.fetch_add(1, Ordering::SeqCst);
/// });
///
/// let result = sync_wait(repeat_n(sender, 5)).unwrap();
/// assert_eq!(result, ());
/// assert_eq!(counter.load(Ordering::SeqCst), 5);
/// ```
pub fn repeat_n<S, E>(sender: S, count: usize) -> RepeatN<S, E>
where
    S: Sender<Completions: CompletionSignatures<Value = (), Error = E>> + Clone,
{
    RepeatN {
        sender,
        count,
        _phantom: PhantomData,
    }
}

/// A sender that repeats another sender N times.
pub struct RepeatN<S, E> {
    sender: S,
    count: usize,
    _phantom: PhantomData<E>,
}

impl<S: Clone, E> Clone for RepeatN<S, E> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            count: self.count,
            _phantom: PhantomData,
        }
    }
}

impl<S, E> std::fmt::Debug for RepeatN<S, E> {
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

impl<S, E> Sender for RepeatN<S, E>
where
    S: Sender<Completions: CompletionSignatures<Value = (), Error = E>> + Clone,
{
    type Completions = RepeatNCompletions<E>;
}

impl<S, E, R> SenderTo<R> for RepeatN<S, E>
where
    S: Sender<Completions: CompletionSignatures<Value = (), Error = E>>
        + SenderTo<RepeatNInnerReceiver<E, R>>
        + Clone
        + Send
        + 'static,
    <S as SenderTo<RepeatNInnerReceiver<E, R>>>::Operation: Send,
    E: Send + 'static,
    R: ReceiverOf<()> + ReceiverOfError<E> + ReceiverOfStopped + Send + 'static,
{
    type Operation = RepeatNOperation<S, E, R>;

    fn connect(self, receiver: R) -> Self::Operation {
        RepeatNOperation {
            state: Arc::new(RepeatNState {
                sender: self.sender,
                remaining: UnsafeCell::new(self.count),
                receiver: UnsafeCell::new(Some(receiver)),
                current_op: UnsafeCell::new(None),
                _phantom: PhantomData,
            }),
        }
    }
}

struct RepeatNState<S, E, R>
where
    S: SenderTo<RepeatNInnerReceiver<E, R>>,
{
    sender: S,
    remaining: UnsafeCell<usize>,
    receiver: UnsafeCell<Option<R>>,
    current_op: UnsafeCell<Option<<S as SenderTo<RepeatNInnerReceiver<E, R>>>::Operation>>,
    _phantom: PhantomData<E>,
}

// SAFETY: Access is synchronized through the execution model
unsafe impl<S: Send, E, R: Send> Send for RepeatNState<S, E, R>
where
    S: SenderTo<RepeatNInnerReceiver<E, R>>,
    <S as SenderTo<RepeatNInnerReceiver<E, R>>>::Operation: Send,
{
}
unsafe impl<S: Send, E, R: Send> Sync for RepeatNState<S, E, R>
where
    S: SenderTo<RepeatNInnerReceiver<E, R>>,
    <S as SenderTo<RepeatNInnerReceiver<E, R>>>::Operation: Send,
{
}

/// Operation state for repeat_n.
pub struct RepeatNOperation<S, E, R>
where
    S: SenderTo<RepeatNInnerReceiver<E, R>>,
{
    state: Arc<RepeatNState<S, E, R>>,
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

impl<S, E, R> RepeatNState<S, E, R>
where
    S: Sender<Completions: CompletionSignatures<Value = (), Error = E>>
        + SenderTo<RepeatNInnerReceiver<E, R>>
        + Clone
        + Send
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

            // Clone the sender for this iteration
            let sender = self.sender.clone();
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
        // Use trampoline to prevent stack overflow with many iterations
        let state = self.clone();
        trampoline_submit(move || {
            state.start_next();
        });
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

impl<S, E, R> OperationState for RepeatNOperation<S, E, R>
where
    S: Sender<Completions: CompletionSignatures<Value = (), Error = E>>
        + SenderTo<RepeatNInnerReceiver<E, R>>
        + Clone
        + Send
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
// repeat_effect_until - repeat until predicate returns true
// ============================================================================

/// Repeat a sender until a predicate returns `true`.
///
/// The sender must implement `Clone` and complete with `()`. After each
/// successful completion, the predicate is called. If it returns `true`,
/// the overall sender completes. If `false`, the sender is cloned and started.
///
/// This follows the stdexec/kuhllib pattern where the sender does work and
/// updates shared state, and the predicate checks that state to determine
/// completion.
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
/// let counter_pred = counter.clone();
///
/// let sender = then(just(()), move |_: ()| {
///     counter_clone.fetch_add(1, Ordering::SeqCst);
/// });
///
/// let predicate = move || counter_pred.load(Ordering::SeqCst) >= 5;
///
/// let result = sync_wait(repeat_effect_until(sender, predicate)).unwrap();
/// assert_eq!(result, ());
/// assert_eq!(counter.load(Ordering::SeqCst), 5);
/// ```
pub fn repeat_effect_until<S, P, E>(sender: S, predicate: P) -> RepeatEffectUntil<S, P, E>
where
    S: Sender<Completions: CompletionSignatures<Value = (), Error = E>> + Clone,
    P: Fn() -> bool,
{
    RepeatEffectUntil {
        sender,
        predicate,
        _phantom: PhantomData,
    }
}

/// A sender that repeats another sender until a predicate returns true.
pub struct RepeatEffectUntil<S, P, E> {
    sender: S,
    predicate: P,
    _phantom: PhantomData<E>,
}

impl<S: Clone, P: Clone, E> Clone for RepeatEffectUntil<S, P, E> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            predicate: self.predicate.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<S, P, E> std::fmt::Debug for RepeatEffectUntil<S, P, E> {
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

impl<S, P, E> Sender for RepeatEffectUntil<S, P, E>
where
    S: Sender<Completions: CompletionSignatures<Value = (), Error = E>> + Clone,
    P: Fn() -> bool,
{
    type Completions = RepeatEffectUntilCompletions<E>;
}

impl<S, P, E, R> SenderTo<R> for RepeatEffectUntil<S, P, E>
where
    S: Sender<Completions: CompletionSignatures<Value = (), Error = E>>
        + SenderTo<RepeatEffectUntilInnerReceiver<E, R>>
        + Clone
        + Send
        + 'static,
    <S as SenderTo<RepeatEffectUntilInnerReceiver<E, R>>>::Operation: Send,
    P: Fn() -> bool + Send + 'static,
    E: Send + 'static,
    R: ReceiverOf<()> + ReceiverOfError<E> + ReceiverOfStopped + Send + 'static,
{
    type Operation = RepeatEffectUntilOperation<S, P, E, R>;

    fn connect(self, receiver: R) -> Self::Operation {
        RepeatEffectUntilOperation {
            state: Arc::new(RepeatEffectUntilState {
                sender: self.sender,
                predicate: self.predicate,
                receiver: UnsafeCell::new(Some(receiver)),
                current_op: UnsafeCell::new(None),
                _phantom: PhantomData,
            }),
        }
    }
}

struct RepeatEffectUntilState<S, P, E, R>
where
    S: SenderTo<RepeatEffectUntilInnerReceiver<E, R>>,
{
    sender: S,
    predicate: P,
    receiver: UnsafeCell<Option<R>>,
    current_op:
        UnsafeCell<Option<<S as SenderTo<RepeatEffectUntilInnerReceiver<E, R>>>::Operation>>,
    _phantom: PhantomData<E>,
}

// SAFETY: Access is synchronized through the execution model
unsafe impl<S: Send, P: Send, E, R: Send> Send for RepeatEffectUntilState<S, P, E, R>
where
    S: SenderTo<RepeatEffectUntilInnerReceiver<E, R>>,
    <S as SenderTo<RepeatEffectUntilInnerReceiver<E, R>>>::Operation: Send,
{
}
unsafe impl<S: Send, P: Send, E, R: Send> Sync for RepeatEffectUntilState<S, P, E, R>
where
    S: SenderTo<RepeatEffectUntilInnerReceiver<E, R>>,
    <S as SenderTo<RepeatEffectUntilInnerReceiver<E, R>>>::Operation: Send,
{
}

/// Operation state for repeat_effect_until.
pub struct RepeatEffectUntilOperation<S, P, E, R>
where
    S: SenderTo<RepeatEffectUntilInnerReceiver<E, R>>,
{
    state: Arc<RepeatEffectUntilState<S, P, E, R>>,
}

/// Inner receiver for repeat_effect_until iterations.
pub struct RepeatEffectUntilInnerReceiver<E, R> {
    on_value: Box<dyn FnOnce(()) + Send>,
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

impl<E, R> ReceiverOf<()> for RepeatEffectUntilInnerReceiver<E, R> {
    fn set_value(self, _: ()) {
        (self.on_value)(());
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

impl<S, P, E, R> RepeatEffectUntilState<S, P, E, R>
where
    S: Sender<Completions: CompletionSignatures<Value = (), Error = E>>
        + SenderTo<RepeatEffectUntilInnerReceiver<E, R>>
        + Clone
        + Send
        + 'static,
    <S as SenderTo<RepeatEffectUntilInnerReceiver<E, R>>>::Operation: Send,
    P: Fn() -> bool + Send + 'static,
    E: Send + 'static,
    R: ReceiverOf<()> + ReceiverOfError<E> + ReceiverOfStopped + Send + 'static,
{
    fn start_iteration(self: &Arc<Self>) {
        let state_for_value = self.clone();
        let state_for_error = self.clone();
        let state_for_stopped = self.clone();

        let inner_receiver = RepeatEffectUntilInnerReceiver {
            on_value: Box::new(move |_| state_for_value.on_value()),
            on_error: Box::new(move |e| state_for_error.on_error(e)),
            on_stopped: Box::new(move || state_for_stopped.on_stopped()),
            _phantom: PhantomData,
        };

        // Clone the sender for this iteration
        let sender = self.sender.clone();
        let op = sender.connect(inner_receiver);
        unsafe {
            *self.current_op.get() = Some(op);
            if let Some(op) = (*self.current_op.get()).as_mut() {
                Pin::new_unchecked(op).start();
            }
        }
    }

    fn on_value(self: &Arc<Self>) {
        // Check predicate after each iteration
        let done = (self.predicate)();
        if done {
            let receiver = unsafe { (*self.receiver.get()).take() };
            if let Some(receiver) = receiver {
                receiver.set_value(());
            }
        } else {
            unsafe {
                *self.current_op.get() = None;
            }
            // Use trampoline to prevent stack overflow with many iterations
            let state = self.clone();
            trampoline_submit(move || {
                state.start_iteration();
            });
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

impl<S, P, E, R> OperationState for RepeatEffectUntilOperation<S, P, E, R>
where
    S: Sender<Completions: CompletionSignatures<Value = (), Error = E>>
        + SenderTo<RepeatEffectUntilInnerReceiver<E, R>>
        + Clone
        + Send
        + 'static,
    <S as SenderTo<RepeatEffectUntilInnerReceiver<E, R>>>::Operation: Send,
    P: Fn() -> bool + Send + 'static,
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

        let sender = then(just(()), move |_: ()| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        });

        let result = sync_wait(repeat_n(sender, 0)).unwrap();
        assert_eq!(result, ());
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_repeat_n_once() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let sender = then(just(()), move |_: ()| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        });

        let result = sync_wait(repeat_n(sender, 1)).unwrap();
        assert_eq!(result, ());
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_repeat_n_multiple() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let sender = then(just(()), move |_: ()| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        });

        let result = sync_wait(repeat_n(sender, 5)).unwrap();
        assert_eq!(result, ());
        assert_eq!(counter.load(Ordering::SeqCst), 5);
    }

    #[test]
    fn test_repeat_effect_until_immediate() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();
        let counter_pred = counter.clone();

        let sender = then(just(()), move |_: ()| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        });

        // Predicate returns true immediately after first iteration
        let predicate = move || counter_pred.load(Ordering::SeqCst) >= 1;

        let result = sync_wait(repeat_effect_until(sender, predicate)).unwrap();
        assert_eq!(result, ());
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_repeat_effect_until_multiple() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();
        let counter_pred = counter.clone();

        let sender = then(just(()), move |_: ()| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        });

        let predicate = move || counter_pred.load(Ordering::SeqCst) >= 5;

        let result = sync_wait(repeat_effect_until(sender, predicate)).unwrap();
        assert_eq!(result, ());
        assert_eq!(counter.load(Ordering::SeqCst), 5);
    }

    #[test]
    fn test_repeat_n_deep_iteration() {
        // Test that deep iteration doesn't overflow the stack
        // thanks to the trampoline scheduler
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let sender = then(just(()), move |_: ()| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        });

        // 10000 iterations would overflow the stack without trampoline
        let result = sync_wait(repeat_n(sender, 10000)).unwrap();
        assert_eq!(result, ());
        assert_eq!(counter.load(Ordering::SeqCst), 10000);
    }

    #[test]
    fn test_repeat_effect_until_deep_iteration() {
        // Test that deep iteration doesn't overflow the stack
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();
        let counter_pred = counter.clone();

        let sender = then(just(()), move |_: ()| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        });

        let predicate = move || counter_pred.load(Ordering::SeqCst) >= 10000;

        let result = sync_wait(repeat_effect_until(sender, predicate)).unwrap();
        assert_eq!(result, ());
        assert_eq!(counter.load(Ordering::SeqCst), 10000);
    }
}
