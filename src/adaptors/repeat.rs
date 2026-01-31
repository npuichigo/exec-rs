//! Repeat adaptors - repeat sender execution.
//!
//! All repeat adaptors take a factory function that creates senders:
//! - `repeat_effect(factory)` - repeat forever (until error/stopped)
//! - `repeat_n(factory, n)` - repeat N times
//! - `repeat_effect_until(factory, predicate)` - repeat until predicate returns true
//!
//! Using factories instead of Clone is more idiomatic Rust:
//! - Explicit about when senders are created
//! - No hidden Clone calls
//! - More flexible (factory can capture state, create different senders per iteration)
//!
//! These implementations are allocation-free, matching C++ stdexec's design.
//! The state lives inline in the operation and receivers hold raw pointers.
//! A trampoline scheduler prevents stack overflow for deep iterations.

use crate::schedulers::trampoline_submit;
use crate::traits::{
    CompletionSignatures, Environment, OperationState, Receiver, ReceiverOf, ReceiverOfError,
    ReceiverOfStopped, Sender, SenderTo,
};
use std::cell::UnsafeCell;
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::pin::Pin;

/// Environment for repeat receivers.
pub struct RepeatEnv;

impl Environment for RepeatEnv {
    type StopToken = crate::stop::NeverStopToken;

    fn get_stop_token(&self) -> Self::StopToken {
        crate::stop::NeverStopToken
    }
}

// ============================================================================
// repeat_n - repeat N times using a factory (ALLOCATION-FREE)
// ============================================================================

/// Repeat N times using a factory function to create each sender.
///
/// The factory receives the iteration index (0-indexed). Each sender runs
/// sequentially after the previous completes, so delays naturally accumulate.
///
/// This implementation is allocation-free - the state lives inline and
/// receivers hold raw pointers to the pinned operation.
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
/// let c = counter.clone();
///
/// let sender = repeat_n(move |_i| {
///     let c = c.clone();
///     then(just(()), move |_| { c.fetch_add(1, Ordering::SeqCst); })
/// }, 5);
///
/// sync_wait(sender).unwrap();
/// assert_eq!(counter.load(Ordering::SeqCst), 5);
/// ```
pub fn repeat_n<F, S, E>(factory: F, count: usize) -> RepeatN<F, S, E>
where
    F: Fn(usize) -> S,
    S: Sender<Completions: CompletionSignatures<Value = (), Error = E>>,
{
    RepeatN {
        factory,
        count,
        _phantom: PhantomData,
    }
}

/// A sender that repeats using a factory function N times.
pub struct RepeatN<F, S, E> {
    factory: F,
    count: usize,
    _phantom: PhantomData<(S, E)>,
}

impl<F: Clone, S, E> Clone for RepeatN<F, S, E> {
    fn clone(&self) -> Self {
        Self {
            factory: self.factory.clone(),
            count: self.count,
            _phantom: PhantomData,
        }
    }
}

impl<F, S, E> std::fmt::Debug for RepeatN<F, S, E> {
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

impl<F, S, E> Sender for RepeatN<F, S, E>
where
    F: Fn(usize) -> S,
    S: Sender<Completions: CompletionSignatures<Value = (), Error = E>>,
{
    type Completions = RepeatNCompletions<E>;
}

/// Inner receiver for repeat_n iterations.
///
/// Uses type-erased function pointers to break the circular type dependency.
/// This is allocation-free - no Box or Arc.
pub struct RepeatNInnerReceiver<E> {
    /// Type-erased pointer to parent operation.
    parent: *mut (),
    /// Callback for value completion - schedules next iteration.
    on_value: unsafe fn(*mut ()),
    /// Callback for error completion - forwards to downstream receiver.
    on_error: unsafe fn(*mut (), E),
    /// Callback for stopped completion - forwards to downstream receiver.
    on_stopped: unsafe fn(*mut ()),
}

// SAFETY: The parent pointer points to a pinned operation that is Send.
unsafe impl<E: Send> Send for RepeatNInnerReceiver<E> {}
unsafe impl<E: Send> Sync for RepeatNInnerReceiver<E> {}

impl<E> Receiver for RepeatNInnerReceiver<E> {
    type Env = RepeatEnv;

    fn get_env(&self) -> &Self::Env {
        &RepeatEnv
    }
}

impl<E> ReceiverOf<()> for RepeatNInnerReceiver<E> {
    fn set_value(self, _: ()) {
        // SAFETY: parent is valid because operation is pinned and outlives us
        unsafe { (self.on_value)(self.parent) }
    }
}

impl<E> ReceiverOfError<E> for RepeatNInnerReceiver<E> {
    fn set_error(self, error: E) {
        unsafe { (self.on_error)(self.parent, error) }
    }
}

impl<E> ReceiverOfStopped for RepeatNInnerReceiver<E> {
    fn set_stopped(self) {
        unsafe { (self.on_stopped)(self.parent) }
    }
}

impl<F, S, E, R> SenderTo<R> for RepeatN<F, S, E>
where
    F: Fn(usize) -> S + Send + 'static,
    S: Sender<Completions: CompletionSignatures<Value = (), Error = E>>
        + SenderTo<RepeatNInnerReceiver<E>>
        + Send
        + 'static,
    <S as SenderTo<RepeatNInnerReceiver<E>>>::Operation: Send,
    E: Send + 'static,
    R: ReceiverOf<()> + ReceiverOfError<E> + ReceiverOfStopped + Send + 'static,
{
    type Operation = RepeatNOperation<F, S, E, R>;

    fn connect(self, receiver: R) -> Self::Operation {
        RepeatNOperation {
            factory: self.factory,
            count: self.count,
            current_index: 0,
            receiver: Some(receiver),
            current_op: UnsafeCell::new(MaybeUninit::uninit()),
            started: false,
            iteration_active: false,
            _phantom: PhantomData,
        }
    }
}

/// Operation state for repeat_n - allocation-free.
///
/// State is stored inline. The inner receiver holds a raw pointer to this
/// operation, which is safe because the operation is pinned when start() is called.
pub struct RepeatNOperation<F, S, E, R>
where
    S: SenderTo<RepeatNInnerReceiver<E>>,
{
    /// Factory to create senders for each iteration.
    factory: F,
    /// Total number of iterations.
    count: usize,
    /// Current iteration index.
    current_index: usize,
    /// Downstream receiver - consumed on completion.
    receiver: Option<R>,
    /// Current child operation - recreated each iteration.
    current_op: UnsafeCell<MaybeUninit<<S as SenderTo<RepeatNInnerReceiver<E>>>::Operation>>,
    /// Whether start() has been called.
    started: bool,
    /// Whether there's an active iteration (child op is initialized).
    iteration_active: bool,
    _phantom: PhantomData<E>,
}

// SAFETY: All fields are Send, and UnsafeCell access is synchronized by the execution model.
unsafe impl<F, S, E, R> Send for RepeatNOperation<F, S, E, R>
where
    F: Send,
    S: SenderTo<RepeatNInnerReceiver<E>> + Send,
    <S as SenderTo<RepeatNInnerReceiver<E>>>::Operation: Send,
    E: Send,
    R: Send,
{
}

impl<F, S, E, R> RepeatNOperation<F, S, E, R>
where
    F: Fn(usize) -> S + Send + 'static,
    S: Sender<Completions: CompletionSignatures<Value = (), Error = E>>
        + SenderTo<RepeatNInnerReceiver<E>>
        + Send
        + 'static,
    <S as SenderTo<RepeatNInnerReceiver<E>>>::Operation: Send,
    E: Send + 'static,
    R: ReceiverOf<()> + ReceiverOfError<E> + ReceiverOfStopped + Send + 'static,
{
    /// Start the next iteration or complete if done.
    ///
    /// SAFETY: Must only be called when the operation is pinned.
    unsafe fn start_next(&mut self) {
        if self.current_index >= self.count {
            // All iterations complete - signal value to downstream
            if let Some(receiver) = self.receiver.take() {
                receiver.set_value(());
            }
            return;
        }

        // Create sender for this iteration
        let sender = (self.factory)(self.current_index);

        // Create inner receiver with raw pointer to self
        let parent = self as *mut Self as *mut ();
        let inner_receiver = RepeatNInnerReceiver {
            parent,
            on_value: Self::handle_value,
            on_error: Self::handle_error,
            on_stopped: Self::handle_stopped,
        };

        // Connect and store the operation
        let op = sender.connect(inner_receiver);
        // SAFETY: We have exclusive access to current_op
        unsafe {
            (*self.current_op.get()).write(op);
        }
        self.iteration_active = true;

        // Start the child operation
        // SAFETY: We have exclusive access and the operation is now initialized
        unsafe {
            let op_ptr = (*self.current_op.get()).as_mut_ptr();
            Pin::new_unchecked(&mut *op_ptr).start();
        }
    }

    /// Handle value completion from child - schedule next iteration.
    ///
    /// SAFETY: parent must be a valid pointer to a pinned RepeatNOperation.
    unsafe fn handle_value(parent: *mut ()) {
        // SAFETY: parent is valid because operation is pinned and outlives us
        let this = unsafe { &mut *(parent as *mut Self) };

        // Drop current operation
        if this.iteration_active {
            // SAFETY: current_op is initialized when iteration_active is true
            unsafe {
                std::ptr::drop_in_place((*this.current_op.get()).as_mut_ptr());
            }
            this.iteration_active = false;
        }

        // Advance to next iteration
        this.current_index += 1;

        // Use trampoline to prevent stack overflow
        let parent_ptr = parent;
        trampoline_submit(move || {
            // SAFETY: parent is valid because operation is pinned and outlives the trampoline
            let this = unsafe { &mut *(parent_ptr as *mut Self) };
            // SAFETY: operation is pinned
            unsafe { this.start_next() };
        });
    }

    /// Handle error completion from child - forward to downstream.
    ///
    /// SAFETY: parent must be a valid pointer to a pinned RepeatNOperation.
    unsafe fn handle_error(parent: *mut (), error: E) {
        // SAFETY: parent is valid because operation is pinned and outlives us
        let this = unsafe { &mut *(parent as *mut Self) };

        // Drop current operation
        if this.iteration_active {
            // SAFETY: current_op is initialized when iteration_active is true
            unsafe {
                std::ptr::drop_in_place((*this.current_op.get()).as_mut_ptr());
            }
            this.iteration_active = false;
        }

        // Forward error to downstream receiver
        if let Some(receiver) = this.receiver.take() {
            receiver.set_error(error);
        }
    }

    /// Handle stopped completion from child - forward to downstream.
    ///
    /// SAFETY: parent must be a valid pointer to a pinned RepeatNOperation.
    unsafe fn handle_stopped(parent: *mut ()) {
        // SAFETY: parent is valid because operation is pinned and outlives us
        let this = unsafe { &mut *(parent as *mut Self) };

        // Drop current operation
        if this.iteration_active {
            // SAFETY: current_op is initialized when iteration_active is true
            unsafe {
                std::ptr::drop_in_place((*this.current_op.get()).as_mut_ptr());
            }
            this.iteration_active = false;
        }

        // Forward stopped to downstream receiver
        if let Some(receiver) = this.receiver.take() {
            receiver.set_stopped();
        }
    }
}

impl<F, S, E, R> OperationState for RepeatNOperation<F, S, E, R>
where
    F: Fn(usize) -> S + Send + 'static,
    S: Sender<Completions: CompletionSignatures<Value = (), Error = E>>
        + SenderTo<RepeatNInnerReceiver<E>>
        + Send
        + 'static,
    <S as SenderTo<RepeatNInnerReceiver<E>>>::Operation: Send,
    E: Send + 'static,
    R: ReceiverOf<()> + ReceiverOfError<E> + ReceiverOfStopped + Send + 'static,
{
    fn start(self: Pin<&mut Self>) {
        // SAFETY: We don't move self, only access fields
        let this = unsafe { self.get_unchecked_mut() };

        if this.started {
            return;
        }
        this.started = true;

        // SAFETY: We're now pinned, so it's safe to store pointers to self
        unsafe { this.start_next() };
    }
}

impl<F, S, E, R> Drop for RepeatNOperation<F, S, E, R>
where
    S: SenderTo<RepeatNInnerReceiver<E>>,
{
    fn drop(&mut self) {
        // Drop the child operation if it's active
        if self.iteration_active {
            unsafe {
                std::ptr::drop_in_place((*self.current_op.get()).as_mut_ptr());
            }
        }
    }
}

// ============================================================================
// repeat_effect - repeat a sender forever using a factory (ALLOCATION-FREE)
// ============================================================================

/// Repeat a sender forever (until error or stopped) using a factory function.
///
/// The factory is called for each iteration to create a new sender.
/// This is useful for accept loops that should run continuously.
///
/// This implementation is allocation-free.
///
/// # Example
///
/// ```rust,ignore
/// use exec_rs::adaptors::repeat_effect;
/// use exec_rs::factories::just;
///
/// // This would run forever
/// let sender = repeat_effect(|| {
///     just(())  // Create a new sender each iteration
/// });
/// ```
pub fn repeat_effect<F, S, E>(factory: F) -> RepeatEffect<F, S, E>
where
    F: Fn() -> S,
    S: Sender<Completions: CompletionSignatures<Value = (), Error = E>>,
{
    RepeatEffect {
        factory,
        _phantom: PhantomData,
    }
}

/// A sender that repeats using a factory forever.
pub struct RepeatEffect<F, S, E> {
    factory: F,
    _phantom: PhantomData<(S, E)>,
}

impl<F: Clone, S, E> Clone for RepeatEffect<F, S, E> {
    fn clone(&self) -> Self {
        Self {
            factory: self.factory.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<F, S, E> std::fmt::Debug for RepeatEffect<F, S, E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RepeatEffect").finish()
    }
}

/// Completion signatures for repeat_effect.
pub struct RepeatEffectCompletions<E> {
    _phantom: PhantomData<E>,
}

impl<E> CompletionSignatures for RepeatEffectCompletions<E> {
    type Value = (); // Never actually completes with value
    type Error = E;
    const SENDS_STOPPED: bool = true;
}

impl<F, S, E> Sender for RepeatEffect<F, S, E>
where
    F: Fn() -> S,
    S: Sender<Completions: CompletionSignatures<Value = (), Error = E>>,
{
    type Completions = RepeatEffectCompletions<E>;
}

/// Inner receiver for repeat_effect iterations.
pub struct RepeatEffectInnerReceiver<E> {
    parent: *mut (),
    on_value: unsafe fn(*mut ()),
    on_error: unsafe fn(*mut (), E),
    on_stopped: unsafe fn(*mut ()),
}

unsafe impl<E: Send> Send for RepeatEffectInnerReceiver<E> {}
unsafe impl<E: Send> Sync for RepeatEffectInnerReceiver<E> {}

impl<E> Receiver for RepeatEffectInnerReceiver<E> {
    type Env = RepeatEnv;

    fn get_env(&self) -> &Self::Env {
        &RepeatEnv
    }
}

impl<E> ReceiverOf<()> for RepeatEffectInnerReceiver<E> {
    fn set_value(self, _: ()) {
        unsafe { (self.on_value)(self.parent) }
    }
}

impl<E> ReceiverOfError<E> for RepeatEffectInnerReceiver<E> {
    fn set_error(self, error: E) {
        unsafe { (self.on_error)(self.parent, error) }
    }
}

impl<E> ReceiverOfStopped for RepeatEffectInnerReceiver<E> {
    fn set_stopped(self) {
        unsafe { (self.on_stopped)(self.parent) }
    }
}

impl<F, S, E, R> SenderTo<R> for RepeatEffect<F, S, E>
where
    F: Fn() -> S + Send + 'static,
    S: Sender<Completions: CompletionSignatures<Value = (), Error = E>>
        + SenderTo<RepeatEffectInnerReceiver<E>>
        + Send
        + 'static,
    <S as SenderTo<RepeatEffectInnerReceiver<E>>>::Operation: Send,
    E: Send + 'static,
    R: ReceiverOf<()> + ReceiverOfError<E> + ReceiverOfStopped + Send + 'static,
{
    type Operation = RepeatEffectOperation<F, S, E, R>;

    fn connect(self, receiver: R) -> Self::Operation {
        RepeatEffectOperation {
            factory: self.factory,
            receiver: Some(receiver),
            current_op: UnsafeCell::new(MaybeUninit::uninit()),
            started: false,
            iteration_active: false,
            _phantom: PhantomData,
        }
    }
}

/// Operation state for repeat_effect - allocation-free.
pub struct RepeatEffectOperation<F, S, E, R>
where
    S: SenderTo<RepeatEffectInnerReceiver<E>>,
{
    factory: F,
    receiver: Option<R>,
    current_op: UnsafeCell<MaybeUninit<<S as SenderTo<RepeatEffectInnerReceiver<E>>>::Operation>>,
    started: bool,
    iteration_active: bool,
    _phantom: PhantomData<E>,
}

unsafe impl<F, S, E, R> Send for RepeatEffectOperation<F, S, E, R>
where
    F: Send,
    S: SenderTo<RepeatEffectInnerReceiver<E>> + Send,
    <S as SenderTo<RepeatEffectInnerReceiver<E>>>::Operation: Send,
    E: Send,
    R: Send,
{
}

impl<F, S, E, R> RepeatEffectOperation<F, S, E, R>
where
    F: Fn() -> S + Send + 'static,
    S: Sender<Completions: CompletionSignatures<Value = (), Error = E>>
        + SenderTo<RepeatEffectInnerReceiver<E>>
        + Send
        + 'static,
    <S as SenderTo<RepeatEffectInnerReceiver<E>>>::Operation: Send,
    E: Send + 'static,
    R: ReceiverOf<()> + ReceiverOfError<E> + ReceiverOfStopped + Send + 'static,
{
    unsafe fn start_iteration(&mut self) {
        let sender = (self.factory)();

        let parent = self as *mut Self as *mut ();
        let inner_receiver = RepeatEffectInnerReceiver {
            parent,
            on_value: Self::handle_value,
            on_error: Self::handle_error,
            on_stopped: Self::handle_stopped,
        };

        let op = sender.connect(inner_receiver);
        // SAFETY: We have exclusive access to current_op
        unsafe {
            (*self.current_op.get()).write(op);
        }
        self.iteration_active = true;

        // SAFETY: We have exclusive access and the operation is now initialized
        unsafe {
            let op_ptr = (*self.current_op.get()).as_mut_ptr();
            Pin::new_unchecked(&mut *op_ptr).start();
        }
    }

    unsafe fn handle_value(parent: *mut ()) {
        // SAFETY: parent is valid because operation is pinned and outlives us
        let this = unsafe { &mut *(parent as *mut Self) };

        if this.iteration_active {
            // SAFETY: current_op is initialized when iteration_active is true
            unsafe {
                std::ptr::drop_in_place((*this.current_op.get()).as_mut_ptr());
            }
            this.iteration_active = false;
        }

        let parent_ptr = parent;
        trampoline_submit(move || {
            // SAFETY: parent is valid because operation is pinned and outlives the trampoline
            let this = unsafe { &mut *(parent_ptr as *mut Self) };
            // SAFETY: operation is pinned
            unsafe { this.start_iteration() };
        });
    }

    unsafe fn handle_error(parent: *mut (), error: E) {
        // SAFETY: parent is valid because operation is pinned and outlives us
        let this = unsafe { &mut *(parent as *mut Self) };

        if this.iteration_active {
            // SAFETY: current_op is initialized when iteration_active is true
            unsafe {
                std::ptr::drop_in_place((*this.current_op.get()).as_mut_ptr());
            }
            this.iteration_active = false;
        }

        if let Some(receiver) = this.receiver.take() {
            receiver.set_error(error);
        }
    }

    unsafe fn handle_stopped(parent: *mut ()) {
        // SAFETY: parent is valid because operation is pinned and outlives us
        let this = unsafe { &mut *(parent as *mut Self) };

        if this.iteration_active {
            // SAFETY: current_op is initialized when iteration_active is true
            unsafe {
                std::ptr::drop_in_place((*this.current_op.get()).as_mut_ptr());
            }
            this.iteration_active = false;
        }

        if let Some(receiver) = this.receiver.take() {
            receiver.set_stopped();
        }
    }
}

impl<F, S, E, R> OperationState for RepeatEffectOperation<F, S, E, R>
where
    F: Fn() -> S + Send + 'static,
    S: Sender<Completions: CompletionSignatures<Value = (), Error = E>>
        + SenderTo<RepeatEffectInnerReceiver<E>>
        + Send
        + 'static,
    <S as SenderTo<RepeatEffectInnerReceiver<E>>>::Operation: Send,
    E: Send + 'static,
    R: ReceiverOf<()> + ReceiverOfError<E> + ReceiverOfStopped + Send + 'static,
{
    fn start(self: Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };

        if this.started {
            return;
        }
        this.started = true;

        unsafe { this.start_iteration() };
    }
}

impl<F, S, E, R> Drop for RepeatEffectOperation<F, S, E, R>
where
    S: SenderTo<RepeatEffectInnerReceiver<E>>,
{
    fn drop(&mut self) {
        if self.iteration_active {
            unsafe {
                std::ptr::drop_in_place((*self.current_op.get()).as_mut_ptr());
            }
        }
    }
}

// ============================================================================
// repeat_effect_until - repeat until predicate returns true (ALLOCATION-FREE)
// ============================================================================

/// Repeat using a factory until a predicate returns `true`.
///
/// The factory is called for each iteration to create a new sender.
/// After each successful completion, the predicate is called. If it returns
/// `true`, the overall sender completes. If `false`, the factory is called
/// again for the next iteration.
///
/// This implementation is allocation-free.
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
/// let c = counter.clone();
/// let c_pred = counter.clone();
///
/// let sender = repeat_effect_until(
///     move || {
///         let c = c.clone();
///         then(just(()), move |_| { c.fetch_add(1, Ordering::SeqCst); })
///     },
///     move || c_pred.load(Ordering::SeqCst) >= 5,
/// );
///
/// sync_wait(sender).unwrap();
/// assert_eq!(counter.load(Ordering::SeqCst), 5);
/// ```
pub fn repeat_effect_until<F, S, P, E>(factory: F, predicate: P) -> RepeatEffectUntil<F, S, P, E>
where
    F: Fn() -> S,
    S: Sender<Completions: CompletionSignatures<Value = (), Error = E>>,
    P: Fn() -> bool,
{
    RepeatEffectUntil {
        factory,
        predicate,
        _phantom: PhantomData,
    }
}

/// A sender that repeats using a factory until a predicate returns true.
pub struct RepeatEffectUntil<F, S, P, E> {
    factory: F,
    predicate: P,
    _phantom: PhantomData<(S, E)>,
}

impl<F: Clone, S, P: Clone, E> Clone for RepeatEffectUntil<F, S, P, E> {
    fn clone(&self) -> Self {
        Self {
            factory: self.factory.clone(),
            predicate: self.predicate.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<F, S, P, E> std::fmt::Debug for RepeatEffectUntil<F, S, P, E> {
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

impl<F, S, P, E> Sender for RepeatEffectUntil<F, S, P, E>
where
    F: Fn() -> S,
    S: Sender<Completions: CompletionSignatures<Value = (), Error = E>>,
    P: Fn() -> bool,
{
    type Completions = RepeatEffectUntilCompletions<E>;
}

/// Inner receiver for repeat_effect_until iterations.
pub struct RepeatEffectUntilInnerReceiver<E> {
    parent: *mut (),
    on_value: unsafe fn(*mut ()),
    on_error: unsafe fn(*mut (), E),
    on_stopped: unsafe fn(*mut ()),
}

unsafe impl<E: Send> Send for RepeatEffectUntilInnerReceiver<E> {}
unsafe impl<E: Send> Sync for RepeatEffectUntilInnerReceiver<E> {}

impl<E> Receiver for RepeatEffectUntilInnerReceiver<E> {
    type Env = RepeatEnv;

    fn get_env(&self) -> &Self::Env {
        &RepeatEnv
    }
}

impl<E> ReceiverOf<()> for RepeatEffectUntilInnerReceiver<E> {
    fn set_value(self, _: ()) {
        unsafe { (self.on_value)(self.parent) }
    }
}

impl<E> ReceiverOfError<E> for RepeatEffectUntilInnerReceiver<E> {
    fn set_error(self, error: E) {
        unsafe { (self.on_error)(self.parent, error) }
    }
}

impl<E> ReceiverOfStopped for RepeatEffectUntilInnerReceiver<E> {
    fn set_stopped(self) {
        unsafe { (self.on_stopped)(self.parent) }
    }
}

impl<F, S, P, E, R> SenderTo<R> for RepeatEffectUntil<F, S, P, E>
where
    F: Fn() -> S + Send + 'static,
    S: Sender<Completions: CompletionSignatures<Value = (), Error = E>>
        + SenderTo<RepeatEffectUntilInnerReceiver<E>>
        + Send
        + 'static,
    <S as SenderTo<RepeatEffectUntilInnerReceiver<E>>>::Operation: Send,
    P: Fn() -> bool + Send + 'static,
    E: Send + 'static,
    R: ReceiverOf<()> + ReceiverOfError<E> + ReceiverOfStopped + Send + 'static,
{
    type Operation = RepeatEffectUntilOperation<F, S, P, E, R>;

    fn connect(self, receiver: R) -> Self::Operation {
        RepeatEffectUntilOperation {
            factory: self.factory,
            predicate: self.predicate,
            receiver: Some(receiver),
            current_op: UnsafeCell::new(MaybeUninit::uninit()),
            started: false,
            iteration_active: false,
            _phantom: PhantomData,
        }
    }
}

/// Operation state for repeat_effect_until - allocation-free.
pub struct RepeatEffectUntilOperation<F, S, P, E, R>
where
    S: SenderTo<RepeatEffectUntilInnerReceiver<E>>,
{
    factory: F,
    predicate: P,
    receiver: Option<R>,
    current_op:
        UnsafeCell<MaybeUninit<<S as SenderTo<RepeatEffectUntilInnerReceiver<E>>>::Operation>>,
    started: bool,
    iteration_active: bool,
    _phantom: PhantomData<E>,
}

unsafe impl<F, S, P, E, R> Send for RepeatEffectUntilOperation<F, S, P, E, R>
where
    F: Send,
    S: SenderTo<RepeatEffectUntilInnerReceiver<E>> + Send,
    <S as SenderTo<RepeatEffectUntilInnerReceiver<E>>>::Operation: Send,
    P: Send,
    E: Send,
    R: Send,
{
}

impl<F, S, P, E, R> RepeatEffectUntilOperation<F, S, P, E, R>
where
    F: Fn() -> S + Send + 'static,
    S: Sender<Completions: CompletionSignatures<Value = (), Error = E>>
        + SenderTo<RepeatEffectUntilInnerReceiver<E>>
        + Send
        + 'static,
    <S as SenderTo<RepeatEffectUntilInnerReceiver<E>>>::Operation: Send,
    P: Fn() -> bool + Send + 'static,
    E: Send + 'static,
    R: ReceiverOf<()> + ReceiverOfError<E> + ReceiverOfStopped + Send + 'static,
{
    unsafe fn start_iteration(&mut self) {
        let sender = (self.factory)();

        let parent = self as *mut Self as *mut ();
        let inner_receiver = RepeatEffectUntilInnerReceiver {
            parent,
            on_value: Self::handle_value,
            on_error: Self::handle_error,
            on_stopped: Self::handle_stopped,
        };

        let op = sender.connect(inner_receiver);
        // SAFETY: We have exclusive access to current_op
        unsafe {
            (*self.current_op.get()).write(op);
        }
        self.iteration_active = true;

        // SAFETY: We have exclusive access and the operation is now initialized
        unsafe {
            let op_ptr = (*self.current_op.get()).as_mut_ptr();
            Pin::new_unchecked(&mut *op_ptr).start();
        }
    }

    unsafe fn handle_value(parent: *mut ()) {
        // SAFETY: parent is valid because operation is pinned and outlives us
        let this = unsafe { &mut *(parent as *mut Self) };

        if this.iteration_active {
            // SAFETY: current_op is initialized when iteration_active is true
            unsafe {
                std::ptr::drop_in_place((*this.current_op.get()).as_mut_ptr());
            }
            this.iteration_active = false;
        }

        // Check predicate
        if (this.predicate)() {
            // Done - signal value to downstream
            if let Some(receiver) = this.receiver.take() {
                receiver.set_value(());
            }
        } else {
            // Continue iterating
            let parent_ptr = parent;
            trampoline_submit(move || {
                // SAFETY: parent is valid because operation is pinned and outlives the trampoline
                let this = unsafe { &mut *(parent_ptr as *mut Self) };
                // SAFETY: operation is pinned
                unsafe { this.start_iteration() };
            });
        }
    }

    unsafe fn handle_error(parent: *mut (), error: E) {
        // SAFETY: parent is valid because operation is pinned and outlives us
        let this = unsafe { &mut *(parent as *mut Self) };

        if this.iteration_active {
            // SAFETY: current_op is initialized when iteration_active is true
            unsafe {
                std::ptr::drop_in_place((*this.current_op.get()).as_mut_ptr());
            }
            this.iteration_active = false;
        }

        if let Some(receiver) = this.receiver.take() {
            receiver.set_error(error);
        }
    }

    unsafe fn handle_stopped(parent: *mut ()) {
        // SAFETY: parent is valid because operation is pinned and outlives us
        let this = unsafe { &mut *(parent as *mut Self) };

        if this.iteration_active {
            // SAFETY: current_op is initialized when iteration_active is true
            unsafe {
                std::ptr::drop_in_place((*this.current_op.get()).as_mut_ptr());
            }
            this.iteration_active = false;
        }

        if let Some(receiver) = this.receiver.take() {
            receiver.set_stopped();
        }
    }
}

impl<F, S, P, E, R> OperationState for RepeatEffectUntilOperation<F, S, P, E, R>
where
    F: Fn() -> S + Send + 'static,
    S: Sender<Completions: CompletionSignatures<Value = (), Error = E>>
        + SenderTo<RepeatEffectUntilInnerReceiver<E>>
        + Send
        + 'static,
    <S as SenderTo<RepeatEffectUntilInnerReceiver<E>>>::Operation: Send,
    P: Fn() -> bool + Send + 'static,
    E: Send + 'static,
    R: ReceiverOf<()> + ReceiverOfError<E> + ReceiverOfStopped + Send + 'static,
{
    fn start(self: Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };

        if this.started {
            return;
        }
        this.started = true;

        unsafe { this.start_iteration() };
    }
}

impl<F, S, P, E, R> Drop for RepeatEffectUntilOperation<F, S, P, E, R>
where
    S: SenderTo<RepeatEffectUntilInnerReceiver<E>>,
{
    fn drop(&mut self) {
        if self.iteration_active {
            unsafe {
                std::ptr::drop_in_place((*self.current_op.get()).as_mut_ptr());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adaptors::then;
    use crate::consumers::sync_wait;
    use crate::factories::just;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn test_repeat_n_zero() {
        let counter = Arc::new(AtomicUsize::new(0));
        let c = counter.clone();

        let sender = repeat_n(
            move |_| {
                let c = c.clone();
                then(just(()), move |_| {
                    c.fetch_add(1, Ordering::SeqCst);
                })
            },
            0,
        );

        let result = sync_wait(sender).unwrap();
        assert_eq!(result, ());
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_repeat_n_once() {
        let counter = Arc::new(AtomicUsize::new(0));
        let c = counter.clone();

        let sender = repeat_n(
            move |_| {
                let c = c.clone();
                then(just(()), move |_| {
                    c.fetch_add(1, Ordering::SeqCst);
                })
            },
            1,
        );

        let result = sync_wait(sender).unwrap();
        assert_eq!(result, ());
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_repeat_n_multiple() {
        let counter = Arc::new(AtomicUsize::new(0));
        let c = counter.clone();

        let sender = repeat_n(
            move |_| {
                let c = c.clone();
                then(just(()), move |_| {
                    c.fetch_add(1, Ordering::SeqCst);
                })
            },
            5,
        );

        let result = sync_wait(sender).unwrap();
        assert_eq!(result, ());
        assert_eq!(counter.load(Ordering::SeqCst), 5);
    }

    #[test]
    fn test_repeat_n_with_index() {
        // Verify the index is passed correctly
        let sum = Arc::new(AtomicUsize::new(0));
        let s = sum.clone();

        let sender = repeat_n(
            move |i| {
                let s = s.clone();
                then(just(()), move |_| {
                    s.fetch_add(i, Ordering::SeqCst);
                })
            },
            5,
        );

        sync_wait(sender).unwrap();
        // Sum of 0+1+2+3+4 = 10
        assert_eq!(sum.load(Ordering::SeqCst), 10);
    }

    #[test]
    fn test_repeat_effect_until_immediate() {
        let counter = Arc::new(AtomicUsize::new(0));
        let c = counter.clone();
        let c_pred = counter.clone();

        let sender = repeat_effect_until(
            move || {
                let c = c.clone();
                then(just(()), move |_| {
                    c.fetch_add(1, Ordering::SeqCst);
                })
            },
            move || c_pred.load(Ordering::SeqCst) >= 1,
        );

        let result = sync_wait(sender).unwrap();
        assert_eq!(result, ());
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_repeat_effect_until_multiple() {
        let counter = Arc::new(AtomicUsize::new(0));
        let c = counter.clone();
        let c_pred = counter.clone();

        let sender = repeat_effect_until(
            move || {
                let c = c.clone();
                then(just(()), move |_| {
                    c.fetch_add(1, Ordering::SeqCst);
                })
            },
            move || c_pred.load(Ordering::SeqCst) >= 5,
        );

        let result = sync_wait(sender).unwrap();
        assert_eq!(result, ());
        assert_eq!(counter.load(Ordering::SeqCst), 5);
    }

    #[test]
    fn test_repeat_n_deep_iteration() {
        // Test that deep iteration doesn't overflow the stack
        let counter = Arc::new(AtomicUsize::new(0));
        let c = counter.clone();

        let sender = repeat_n(
            move |_| {
                let c = c.clone();
                then(just(()), move |_| {
                    c.fetch_add(1, Ordering::SeqCst);
                })
            },
            10000,
        );

        let result = sync_wait(sender).unwrap();
        assert_eq!(result, ());
        assert_eq!(counter.load(Ordering::SeqCst), 10000);
    }

    #[test]
    fn test_repeat_effect_until_deep_iteration() {
        // Test that deep iteration doesn't overflow the stack
        let counter = Arc::new(AtomicUsize::new(0));
        let c = counter.clone();
        let c_pred = counter.clone();

        let sender = repeat_effect_until(
            move || {
                let c = c.clone();
                then(just(()), move |_| {
                    c.fetch_add(1, Ordering::SeqCst);
                })
            },
            move || c_pred.load(Ordering::SeqCst) >= 10000,
        );

        let result = sync_wait(sender).unwrap();
        assert_eq!(result, ());
        assert_eq!(counter.load(Ordering::SeqCst), 10000);
    }
}
