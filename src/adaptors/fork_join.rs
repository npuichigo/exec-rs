//! fork_join - Fork a sender's result to multiple downstream computations.
//!
//! `fork_join` runs a predecessor sender, then applies multiple functions
//! to the result, producing a tuple of all the results.
//!
//! This enables fan-out patterns where one sender's result feeds into
//! multiple downstream computations.

use crate::traits::{
    CompletionSignatures, OperationState, Receiver, ReceiverOf, ReceiverOfError, ReceiverOfStopped,
    Sender, SenderTo,
};
use std::marker::PhantomData;
use std::pin::Pin;

/// Fork a sender's value to two computations.
///
/// Runs the predecessor sender, then applies both functions to the result
/// and returns a tuple of both results.
///
/// # Example
///
/// ```rust
/// use exec_rs::factories::just;
/// use exec_rs::adaptors::fork_join2;
/// use exec_rs::consumers::sync_wait;
///
/// let result = sync_wait(fork_join2(
///     just(10),
///     |x| x * 2,  // First branch: multiply by 2
///     |x| x + 5,  // Second branch: add 5
/// )).unwrap();
///
/// assert_eq!(result, (20, 15));
/// ```
pub fn fork_join2<S, F1, F2, V, V1, V2>(predecessor: S, f1: F1, f2: F2) -> ForkJoin2<S, F1, F2>
where
    S: Sender,
    F1: FnOnce(V) -> V1,
    F2: FnOnce(V) -> V2,
    V: Clone,
{
    ForkJoin2 {
        predecessor,
        f1: Some(f1),
        f2: Some(f2),
    }
}

/// Fork a sender's value to three computations.
///
/// Similar to `fork_join2` but with three branches.
///
/// # Example
///
/// ```rust
/// use exec_rs::factories::just;
/// use exec_rs::adaptors::fork_join3;
/// use exec_rs::consumers::sync_wait;
///
/// let result = sync_wait(fork_join3(
///     just(10),
///     |x| x * 2,
///     |x| x + 5,
///     |x| x - 3,
/// )).unwrap();
///
/// assert_eq!(result, (20, 15, 7));
/// ```
pub fn fork_join3<S, F1, F2, F3, V, V1, V2, V3>(
    predecessor: S,
    f1: F1,
    f2: F2,
    f3: F3,
) -> ForkJoin3<S, F1, F2, F3>
where
    S: Sender,
    F1: FnOnce(V) -> V1,
    F2: FnOnce(V) -> V2,
    F3: FnOnce(V) -> V3,
    V: Clone,
{
    ForkJoin3 {
        predecessor,
        f1: Some(f1),
        f2: Some(f2),
        f3: Some(f3),
    }
}

/// Fork a sender's value to four computations.
///
/// Similar to `fork_join2` but with four branches.
pub fn fork_join4<S, F1, F2, F3, F4, V, V1, V2, V3, V4>(
    predecessor: S,
    f1: F1,
    f2: F2,
    f3: F3,
    f4: F4,
) -> ForkJoin4<S, F1, F2, F3, F4>
where
    S: Sender,
    F1: FnOnce(V) -> V1,
    F2: FnOnce(V) -> V2,
    F3: FnOnce(V) -> V3,
    F4: FnOnce(V) -> V4,
    V: Clone,
{
    ForkJoin4 {
        predecessor,
        f1: Some(f1),
        f2: Some(f2),
        f3: Some(f3),
        f4: Some(f4),
    }
}

// ============================================================================
// ForkJoin2
// ============================================================================

/// A sender that forks a value to two computations.
pub struct ForkJoin2<S, F1, F2> {
    predecessor: S,
    f1: Option<F1>,
    f2: Option<F2>,
}

/// Completion signatures for ForkJoin2.
pub struct ForkJoin2Completions<V1, V2, E>(PhantomData<(V1, V2, E)>);

impl<V1, V2, E> CompletionSignatures for ForkJoin2Completions<V1, V2, E> {
    type Value = (V1, V2);
    type Error = E;
    const SENDS_STOPPED: bool = true;
}

impl<S, F1, F2, V, V1, V2, E> Sender for ForkJoin2<S, F1, F2>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
    F1: FnOnce(V) -> V1,
    F2: FnOnce(V) -> V2,
    V: Clone,
{
    type Completions = ForkJoin2Completions<V1, V2, E>;
}

impl<S, F1, F2, R, V, V1, V2, E> SenderTo<R> for ForkJoin2<S, F1, F2>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>
        + SenderTo<ForkJoin2Receiver<R, F1, F2, V, V1, V2, E>>,
    F1: FnOnce(V) -> V1,
    F2: FnOnce(V) -> V2,
    V: Clone,
    R: ReceiverOf<(V1, V2)> + ReceiverOfError<E> + ReceiverOfStopped,
{
    type Operation = ForkJoin2Operation<S, R, F1, F2, V, V1, V2, E>;

    fn connect(mut self, receiver: R) -> Self::Operation {
        let inner_receiver = ForkJoin2Receiver {
            receiver: Some(receiver),
            f1: self.f1.take(),
            f2: self.f2.take(),
            _phantom: PhantomData,
        };
        ForkJoin2Operation {
            inner: self.predecessor.connect(inner_receiver),
        }
    }
}

/// Receiver for ForkJoin2.
pub struct ForkJoin2Receiver<R, F1, F2, V, V1, V2, E> {
    receiver: Option<R>,
    f1: Option<F1>,
    f2: Option<F2>,
    _phantom: PhantomData<(V, V1, V2, E)>,
}

impl<R: Receiver, F1, F2, V, V1, V2, E> Receiver for ForkJoin2Receiver<R, F1, F2, V, V1, V2, E> {
    type Env = R::Env;

    fn get_env(&self) -> &Self::Env {
        self.receiver.as_ref().unwrap().get_env()
    }
}

impl<R, F1, F2, V, V1, V2, E> ReceiverOf<V> for ForkJoin2Receiver<R, F1, F2, V, V1, V2, E>
where
    R: ReceiverOf<(V1, V2)>,
    F1: FnOnce(V) -> V1,
    F2: FnOnce(V) -> V2,
    V: Clone,
{
    fn set_value(mut self, value: V) {
        if let (Some(receiver), Some(f1), Some(f2)) =
            (self.receiver.take(), self.f1.take(), self.f2.take())
        {
            let v1 = f1(value.clone());
            let v2 = f2(value);
            receiver.set_value((v1, v2));
        }
    }
}

impl<R, F1, F2, V, V1, V2, E> ReceiverOfError<E> for ForkJoin2Receiver<R, F1, F2, V, V1, V2, E>
where
    R: ReceiverOfError<E>,
{
    fn set_error(mut self, error: E) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_error(error);
        }
    }
}

impl<R, F1, F2, V, V1, V2, E> ReceiverOfStopped for ForkJoin2Receiver<R, F1, F2, V, V1, V2, E>
where
    R: ReceiverOfStopped,
{
    fn set_stopped(mut self) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_stopped();
        }
    }
}

/// Operation state for ForkJoin2.
pub struct ForkJoin2Operation<S, R, F1, F2, V, V1, V2, E>
where
    S: SenderTo<ForkJoin2Receiver<R, F1, F2, V, V1, V2, E>>,
    R: Receiver,
{
    inner: S::Operation,
}

impl<S, R, F1, F2, V, V1, V2, E> OperationState for ForkJoin2Operation<S, R, F1, F2, V, V1, V2, E>
where
    S: SenderTo<ForkJoin2Receiver<R, F1, F2, V, V1, V2, E>>,
    R: Receiver,
{
    fn start(self: Pin<&mut Self>) {
        unsafe {
            let this = self.get_unchecked_mut();
            Pin::new_unchecked(&mut this.inner).start();
        }
    }
}

// ============================================================================
// ForkJoin3
// ============================================================================

/// A sender that forks a value to three computations.
pub struct ForkJoin3<S, F1, F2, F3> {
    predecessor: S,
    f1: Option<F1>,
    f2: Option<F2>,
    f3: Option<F3>,
}

/// Completion signatures for ForkJoin3.
pub struct ForkJoin3Completions<V1, V2, V3, E>(PhantomData<(V1, V2, V3, E)>);

impl<V1, V2, V3, E> CompletionSignatures for ForkJoin3Completions<V1, V2, V3, E> {
    type Value = (V1, V2, V3);
    type Error = E;
    const SENDS_STOPPED: bool = true;
}

impl<S, F1, F2, F3, V, V1, V2, V3, E> Sender for ForkJoin3<S, F1, F2, F3>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
    F1: FnOnce(V) -> V1,
    F2: FnOnce(V) -> V2,
    F3: FnOnce(V) -> V3,
    V: Clone,
{
    type Completions = ForkJoin3Completions<V1, V2, V3, E>;
}

impl<S, F1, F2, F3, R, V, V1, V2, V3, E> SenderTo<R> for ForkJoin3<S, F1, F2, F3>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>
        + SenderTo<ForkJoin3Receiver<R, F1, F2, F3, V, V1, V2, V3, E>>,
    F1: FnOnce(V) -> V1,
    F2: FnOnce(V) -> V2,
    F3: FnOnce(V) -> V3,
    V: Clone,
    R: ReceiverOf<(V1, V2, V3)> + ReceiverOfError<E> + ReceiverOfStopped,
{
    type Operation = ForkJoin3Operation<S, R, F1, F2, F3, V, V1, V2, V3, E>;

    fn connect(mut self, receiver: R) -> Self::Operation {
        let inner_receiver = ForkJoin3Receiver {
            receiver: Some(receiver),
            f1: self.f1.take(),
            f2: self.f2.take(),
            f3: self.f3.take(),
            _phantom: PhantomData,
        };
        ForkJoin3Operation {
            inner: self.predecessor.connect(inner_receiver),
        }
    }
}

/// Receiver for ForkJoin3.
pub struct ForkJoin3Receiver<R, F1, F2, F3, V, V1, V2, V3, E> {
    receiver: Option<R>,
    f1: Option<F1>,
    f2: Option<F2>,
    f3: Option<F3>,
    _phantom: PhantomData<(V, V1, V2, V3, E)>,
}

impl<R: Receiver, F1, F2, F3, V, V1, V2, V3, E> Receiver
    for ForkJoin3Receiver<R, F1, F2, F3, V, V1, V2, V3, E>
{
    type Env = R::Env;

    fn get_env(&self) -> &Self::Env {
        self.receiver.as_ref().unwrap().get_env()
    }
}

impl<R, F1, F2, F3, V, V1, V2, V3, E> ReceiverOf<V>
    for ForkJoin3Receiver<R, F1, F2, F3, V, V1, V2, V3, E>
where
    R: ReceiverOf<(V1, V2, V3)>,
    F1: FnOnce(V) -> V1,
    F2: FnOnce(V) -> V2,
    F3: FnOnce(V) -> V3,
    V: Clone,
{
    fn set_value(mut self, value: V) {
        if let (Some(receiver), Some(f1), Some(f2), Some(f3)) = (
            self.receiver.take(),
            self.f1.take(),
            self.f2.take(),
            self.f3.take(),
        ) {
            let v1 = f1(value.clone());
            let v2 = f2(value.clone());
            let v3 = f3(value);
            receiver.set_value((v1, v2, v3));
        }
    }
}

impl<R, F1, F2, F3, V, V1, V2, V3, E> ReceiverOfError<E>
    for ForkJoin3Receiver<R, F1, F2, F3, V, V1, V2, V3, E>
where
    R: ReceiverOfError<E>,
{
    fn set_error(mut self, error: E) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_error(error);
        }
    }
}

impl<R, F1, F2, F3, V, V1, V2, V3, E> ReceiverOfStopped
    for ForkJoin3Receiver<R, F1, F2, F3, V, V1, V2, V3, E>
where
    R: ReceiverOfStopped,
{
    fn set_stopped(mut self) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_stopped();
        }
    }
}

/// Operation state for ForkJoin3.
pub struct ForkJoin3Operation<S, R, F1, F2, F3, V, V1, V2, V3, E>
where
    S: SenderTo<ForkJoin3Receiver<R, F1, F2, F3, V, V1, V2, V3, E>>,
    R: Receiver,
{
    inner: S::Operation,
}

impl<S, R, F1, F2, F3, V, V1, V2, V3, E> OperationState
    for ForkJoin3Operation<S, R, F1, F2, F3, V, V1, V2, V3, E>
where
    S: SenderTo<ForkJoin3Receiver<R, F1, F2, F3, V, V1, V2, V3, E>>,
    R: Receiver,
{
    fn start(self: Pin<&mut Self>) {
        unsafe {
            let this = self.get_unchecked_mut();
            Pin::new_unchecked(&mut this.inner).start();
        }
    }
}

// ============================================================================
// ForkJoin4
// ============================================================================

/// A sender that forks a value to four computations.
pub struct ForkJoin4<S, F1, F2, F3, F4> {
    predecessor: S,
    f1: Option<F1>,
    f2: Option<F2>,
    f3: Option<F3>,
    f4: Option<F4>,
}

/// Completion signatures for ForkJoin4.
pub struct ForkJoin4Completions<V1, V2, V3, V4, E>(PhantomData<(V1, V2, V3, V4, E)>);

impl<V1, V2, V3, V4, E> CompletionSignatures for ForkJoin4Completions<V1, V2, V3, V4, E> {
    type Value = (V1, V2, V3, V4);
    type Error = E;
    const SENDS_STOPPED: bool = true;
}

impl<S, F1, F2, F3, F4, V, V1, V2, V3, V4, E> Sender for ForkJoin4<S, F1, F2, F3, F4>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>,
    F1: FnOnce(V) -> V1,
    F2: FnOnce(V) -> V2,
    F3: FnOnce(V) -> V3,
    F4: FnOnce(V) -> V4,
    V: Clone,
{
    type Completions = ForkJoin4Completions<V1, V2, V3, V4, E>;
}

impl<S, F1, F2, F3, F4, R, V, V1, V2, V3, V4, E> SenderTo<R> for ForkJoin4<S, F1, F2, F3, F4>
where
    S: Sender<Completions: CompletionSignatures<Value = V, Error = E>>
        + SenderTo<ForkJoin4Receiver<R, F1, F2, F3, F4, V, V1, V2, V3, V4, E>>,
    F1: FnOnce(V) -> V1,
    F2: FnOnce(V) -> V2,
    F3: FnOnce(V) -> V3,
    F4: FnOnce(V) -> V4,
    V: Clone,
    R: ReceiverOf<(V1, V2, V3, V4)> + ReceiverOfError<E> + ReceiverOfStopped,
{
    type Operation = ForkJoin4Operation<S, R, F1, F2, F3, F4, V, V1, V2, V3, V4, E>;

    fn connect(mut self, receiver: R) -> Self::Operation {
        let inner_receiver = ForkJoin4Receiver {
            receiver: Some(receiver),
            f1: self.f1.take(),
            f2: self.f2.take(),
            f3: self.f3.take(),
            f4: self.f4.take(),
            _phantom: PhantomData,
        };
        ForkJoin4Operation {
            inner: self.predecessor.connect(inner_receiver),
        }
    }
}

/// Receiver for ForkJoin4.
pub struct ForkJoin4Receiver<R, F1, F2, F3, F4, V, V1, V2, V3, V4, E> {
    receiver: Option<R>,
    f1: Option<F1>,
    f2: Option<F2>,
    f3: Option<F3>,
    f4: Option<F4>,
    _phantom: PhantomData<(V, V1, V2, V3, V4, E)>,
}

impl<R: Receiver, F1, F2, F3, F4, V, V1, V2, V3, V4, E> Receiver
    for ForkJoin4Receiver<R, F1, F2, F3, F4, V, V1, V2, V3, V4, E>
{
    type Env = R::Env;

    fn get_env(&self) -> &Self::Env {
        self.receiver.as_ref().unwrap().get_env()
    }
}

impl<R, F1, F2, F3, F4, V, V1, V2, V3, V4, E> ReceiverOf<V>
    for ForkJoin4Receiver<R, F1, F2, F3, F4, V, V1, V2, V3, V4, E>
where
    R: ReceiverOf<(V1, V2, V3, V4)>,
    F1: FnOnce(V) -> V1,
    F2: FnOnce(V) -> V2,
    F3: FnOnce(V) -> V3,
    F4: FnOnce(V) -> V4,
    V: Clone,
{
    fn set_value(mut self, value: V) {
        if let (Some(receiver), Some(f1), Some(f2), Some(f3), Some(f4)) = (
            self.receiver.take(),
            self.f1.take(),
            self.f2.take(),
            self.f3.take(),
            self.f4.take(),
        ) {
            let v1 = f1(value.clone());
            let v2 = f2(value.clone());
            let v3 = f3(value.clone());
            let v4 = f4(value);
            receiver.set_value((v1, v2, v3, v4));
        }
    }
}

impl<R, F1, F2, F3, F4, V, V1, V2, V3, V4, E> ReceiverOfError<E>
    for ForkJoin4Receiver<R, F1, F2, F3, F4, V, V1, V2, V3, V4, E>
where
    R: ReceiverOfError<E>,
{
    fn set_error(mut self, error: E) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_error(error);
        }
    }
}

impl<R, F1, F2, F3, F4, V, V1, V2, V3, V4, E> ReceiverOfStopped
    for ForkJoin4Receiver<R, F1, F2, F3, F4, V, V1, V2, V3, V4, E>
where
    R: ReceiverOfStopped,
{
    fn set_stopped(mut self) {
        if let Some(receiver) = self.receiver.take() {
            receiver.set_stopped();
        }
    }
}

/// Operation state for ForkJoin4.
pub struct ForkJoin4Operation<S, R, F1, F2, F3, F4, V, V1, V2, V3, V4, E>
where
    S: SenderTo<ForkJoin4Receiver<R, F1, F2, F3, F4, V, V1, V2, V3, V4, E>>,
    R: Receiver,
{
    inner: S::Operation,
}

impl<S, R, F1, F2, F3, F4, V, V1, V2, V3, V4, E> OperationState
    for ForkJoin4Operation<S, R, F1, F2, F3, F4, V, V1, V2, V3, V4, E>
where
    S: SenderTo<ForkJoin4Receiver<R, F1, F2, F3, F4, V, V1, V2, V3, V4, E>>,
    R: Receiver,
{
    fn start(self: Pin<&mut Self>) {
        unsafe {
            let this = self.get_unchecked_mut();
            Pin::new_unchecked(&mut this.inner).start();
        }
    }
}

/// Extension trait for fork_join operations.
pub trait ForkJoinExt: Sender + Sized {
    /// Fork this sender's value to two computations.
    fn fork_join2<F1, F2, V, V1, V2>(self, f1: F1, f2: F2) -> ForkJoin2<Self, F1, F2>
    where
        F1: FnOnce(V) -> V1,
        F2: FnOnce(V) -> V2,
        V: Clone,
    {
        fork_join2(self, f1, f2)
    }

    /// Fork this sender's value to three computations.
    fn fork_join3<F1, F2, F3, V, V1, V2, V3>(
        self,
        f1: F1,
        f2: F2,
        f3: F3,
    ) -> ForkJoin3<Self, F1, F2, F3>
    where
        F1: FnOnce(V) -> V1,
        F2: FnOnce(V) -> V2,
        F3: FnOnce(V) -> V3,
        V: Clone,
    {
        fork_join3(self, f1, f2, f3)
    }

    /// Fork this sender's value to four computations.
    fn fork_join4<F1, F2, F3, F4, V, V1, V2, V3, V4>(
        self,
        f1: F1,
        f2: F2,
        f3: F3,
        f4: F4,
    ) -> ForkJoin4<Self, F1, F2, F3, F4>
    where
        F1: FnOnce(V) -> V1,
        F2: FnOnce(V) -> V2,
        F3: FnOnce(V) -> V3,
        F4: FnOnce(V) -> V4,
        V: Clone,
    {
        fork_join4(self, f1, f2, f3, f4)
    }
}

impl<S: Sender> ForkJoinExt for S {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consumers::sync_wait;
    use crate::factories::just;

    #[test]
    fn test_fork_join2_basic() {
        let result = sync_wait(fork_join2(just(10), |x| x * 2, |x| x + 5)).unwrap();

        assert_eq!(result, (20, 15));
    }

    #[test]
    fn test_fork_join2_string() {
        let result = sync_wait(fork_join2(
            just("hello".to_string()),
            |s: String| s.len(),
            |s: String| s.to_uppercase(),
        ))
        .unwrap();

        assert_eq!(result, (5, "HELLO".to_string()));
    }

    #[test]
    fn test_fork_join3_basic() {
        let result = sync_wait(fork_join3(just(10), |x| x * 2, |x| x + 5, |x| x - 3)).unwrap();

        assert_eq!(result, (20, 15, 7));
    }

    #[test]
    fn test_fork_join4_basic() {
        let result = sync_wait(fork_join4(
            just(10),
            |x| x * 2,
            |x| x + 5,
            |x| x - 3,
            |x| x / 2,
        ))
        .unwrap();

        assert_eq!(result, (20, 15, 7, 5));
    }

    #[test]
    fn test_fork_join_extension_trait() {
        let result = sync_wait(just(10).fork_join2(|x| x * 2, |x| x + 5)).unwrap();

        assert_eq!(result, (20, 15));
    }

    #[test]
    fn test_fork_join_with_unit() {
        let result = sync_wait(fork_join2(just(()), |_| 1, |_| 2)).unwrap();

        assert_eq!(result, (1, 2));
    }

    #[test]
    fn test_fork_join2_same_type() {
        let result = sync_wait(fork_join2(just(5), |x| x * 2, |x| x * 3)).unwrap();

        assert_eq!(result, (10, 15));
    }

    #[test]
    fn test_fork_join_chained() {
        use crate::adaptors::then;

        let result = sync_wait(then(
            fork_join2(just(10), |x| x * 2, |x| x + 5),
            |(a, b)| a + b,
        ))
        .unwrap();

        assert_eq!(result, 35); // 20 + 15
    }
}
