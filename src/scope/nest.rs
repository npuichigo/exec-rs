//! Nest adaptor - tracks a sender within a scope.

use super::async_scope::{notify_waiters_if_empty, AsyncScopeInner};
use crate::stop::InplaceStopToken;
use crate::traits::{
    CompletionSignatures, Environment, OperationState, Receiver, ReceiverOf, ReceiverOfError,
    ReceiverOfStopped, Sender, SenderTo,
};
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// A sender that tracks its lifetime within a scope.
///
/// When started, the active count is incremented.
/// When completed (value, error, or stopped), the count is decremented.
pub struct Nest<S> {
    sender: S,
    scope: Arc<AsyncScopeInner>,
}

impl<S> Nest<S> {
    pub(crate) fn new(sender: S, scope: Arc<AsyncScopeInner>) -> Self {
        Self { sender, scope }
    }
}

impl<S: Clone> Clone for Nest<S> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            scope: self.scope.clone(),
        }
    }
}

/// Completion signatures for Nest - same as the inner sender.
pub struct NestCompletions<V, E>(PhantomData<(V, E)>);

impl<V, E> CompletionSignatures for NestCompletions<V, E> {
    type Value = V;
    type Error = E;
    const SENDS_STOPPED: bool = true;
}

impl<S, V, E> Sender for Nest<S>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
{
    type Completions = NestCompletions<V, E>;
}

impl<S, R, V, E> SenderTo<R> for Nest<S>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>
        + SenderTo<NestReceiver<R>>,
    R: ReceiverOf<V> + ReceiverOfError<E> + ReceiverOfStopped,
{
    type Operation = NestOperation<S, R>;

    fn connect(self, receiver: R) -> Self::Operation {
        let nest_receiver = NestReceiver {
            receiver,
            scope: self.scope.clone(),
        };

        NestOperation {
            scope: self.scope,
            inner: self.sender.connect(nest_receiver),
            started: false,
        }
    }
}

/// Receiver that decrements scope count on completion.
pub struct NestReceiver<R> {
    receiver: R,
    scope: Arc<AsyncScopeInner>,
}

/// Environment for nest receivers.
pub struct NestEnv {
    stop_token: InplaceStopToken,
}

impl Environment for NestEnv {
    type StopToken = InplaceStopToken;

    fn get_stop_token(&self) -> Self::StopToken {
        self.stop_token.clone()
    }
}

impl<R: Receiver> Receiver for NestReceiver<R> {
    type Env = NestEnv;

    fn get_env(&self) -> &Self::Env {
        thread_local! {
            static ENV: NestEnv = NestEnv {
                stop_token: InplaceStopToken::new_detached(),
            };
        }
        ENV.with(|env| unsafe { &*(env as *const NestEnv) })
    }
}

impl<R, V> ReceiverOf<V> for NestReceiver<R>
where
    R: ReceiverOf<V>,
{
    fn set_value(self, value: V) {
        let scope = self.scope.clone();
        self.receiver.set_value(value);
        // Decrement after forwarding to avoid issues if receiver panics
        let prev = scope.active.fetch_sub(1, Ordering::AcqRel);
        if prev == 1 {
            notify_waiters_if_empty(&scope);
        }
    }
}

impl<R, E> ReceiverOfError<E> for NestReceiver<R>
where
    R: ReceiverOfError<E>,
{
    fn set_error(self, error: E) {
        let scope = self.scope.clone();
        self.receiver.set_error(error);
        let prev = scope.active.fetch_sub(1, Ordering::AcqRel);
        if prev == 1 {
            notify_waiters_if_empty(&scope);
        }
    }
}

impl<R> ReceiverOfStopped for NestReceiver<R>
where
    R: ReceiverOfStopped,
{
    fn set_stopped(self) {
        let scope = self.scope.clone();
        self.receiver.set_stopped();
        let prev = scope.active.fetch_sub(1, Ordering::AcqRel);
        if prev == 1 {
            notify_waiters_if_empty(&scope);
        }
    }
}

/// Operation state for Nest.
pub struct NestOperation<S, R>
where
    S: SenderTo<NestReceiver<R>>,
    R: Receiver,
{
    scope: Arc<AsyncScopeInner>,
    inner: S::Operation,
    started: bool,
}

impl<S, R> OperationState for NestOperation<S, R>
where
    S: SenderTo<NestReceiver<R>>,
    R: Receiver,
{
    fn start(self: Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };

        if this.started {
            return;
        }
        this.started = true;

        // Increment active count when starting
        this.scope.active.fetch_add(1, Ordering::AcqRel);

        // Start the inner operation
        unsafe {
            Pin::new_unchecked(&mut this.inner).start();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consumers::sync_wait;
    use crate::factories::just;
    use crate::scope::AsyncScope;

    #[test]
    fn test_nest_basic() {
        let scope = AsyncScope::new();
        let nested = scope.nest(just(42));
        let result = sync_wait(nested).unwrap();
        assert_eq!(result, 42);
        assert_eq!(scope.active_count(), 0);
    }

    #[test]
    fn test_nest_string() {
        let scope = AsyncScope::new();
        let nested = scope.nest(just("hello"));
        let result = sync_wait(nested).unwrap();
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_nest_multiple() {
        let scope = AsyncScope::new();
        
        let r1 = sync_wait(scope.nest(just(1))).unwrap();
        let r2 = sync_wait(scope.nest(just(2))).unwrap();
        let r3 = sync_wait(scope.nest(just(3))).unwrap();
        
        assert_eq!(r1, 1);
        assert_eq!(r2, 2);
        assert_eq!(r3, 3);
        assert_eq!(scope.active_count(), 0);
    }
}
