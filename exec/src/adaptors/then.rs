use exec_core::receiver::SetValue;
use exec_core::{OperationState, Sender};
use std::marker::PhantomData;

pub struct Then<S, F> {
    sender: S,
    func: F,
}

impl<S, F> Then<S, F> {
    pub fn new(sender: S, func: F) -> Self {
        Self { sender, func }
    }
}

pub struct ThenReceiver<I, F, R> {
    func: F,
    receiver: R,
    _phantom: PhantomData<I>,
}

impl<I, F, R, O> SetValue for ThenReceiver<I, F, R>
where
    F: FnOnce(I) -> O,
    R: SetValue<Value = O>,
{
    type Value = I;

    fn set_value(self, value: Self::Value) {
        self.receiver.set_value((self.func)(value));
    }
}

pub struct ThenOperation<O> {
    operation: O,
}

impl<O> OperationState for ThenOperation<O>
where
    O: OperationState,
{
    fn start(self) {
        self.operation.start()
    }
}

impl<S, F, R, O> Sender<R> for Then<S, F>
where
    S: Sender<ThenReceiver<S::Output, F, R>>,
    F: FnOnce(S::Output) -> O,
    R: SetValue<Value = O>,
{
    type Output = O;
    type Operation = ThenOperation<S::Operation>;

    fn connect(self, receiver: R) -> Self::Operation {
        ThenOperation {
            operation: self.sender.connect(ThenReceiver {
                func: self.func,
                receiver,
                _phantom: Default::default(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::factories::just::Just;
    use exec_test::receivers::ExpectReceiver;

    #[test]
    fn test_then() {
        let just_sender = Just::new(42);
        let then_sender = Then::new(just_sender, |x| x + 1);
        let operation = then_sender.connect(ExpectReceiver::new(43));
        operation.start();
    }
}
