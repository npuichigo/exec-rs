use crate::consumers::SenderAwaitable;
use exec_core::receiver::SetValue;
use exec_core::{OperationState, Sender};
use std::future::IntoFuture;

pub fn just<T>(value: T) -> Just<T> {
    Just::new(value)
}

pub struct Just<T> {
    value: T,
}

impl<T> Just<T> {
    pub fn new(value: T) -> Self {
        Self { value }
    }
}

pub struct JustOperation<T, R> {
    data: Option<T>,
    receiver: Option<R>,
}

impl<T, R> OperationState for JustOperation<T, R>
where
    R: SetValue<Value = T>,
{
    fn start(&mut self) {
        if let (Some(receiver), Some(data)) = (self.receiver.take(), self.data.take()) {
            receiver.set_value(data);
        }
    }
}

impl<T, R> Sender<R> for Just<T>
where
    R: SetValue<Value = T>,
{
    type Value = R::Value;
    type Error = ();

    type Operation = JustOperation<T, R>;

    fn connect(self, receiver: R) -> Self::Operation {
        JustOperation {
            data: Some(self.value),
            receiver: Some(receiver),
        }
    }
}

impl<T> IntoFuture for Just<T> {
    type Output = Result<Option<T>, ()>;
    type IntoFuture = SenderAwaitable<Self, T, ()>;

    fn into_future(self) -> Self::IntoFuture {
        SenderAwaitable::new(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use exec_test::receivers::ExpectValueReceiver;

    #[test]
    fn test_just() {
        let sender = Just::new(42);
        let mut operation = sender.connect(ExpectValueReceiver::new(42));
        operation.start();
    }
}
