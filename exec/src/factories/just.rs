use exec_core::receiver::SetValue;
use exec_core::{OperationState, Sender};

pub struct Just<T> {
    data: T,
}

impl<T> Just<T> {
    pub fn new(value: T) -> Self {
        Self { data: value }
    }
}

pub struct JustOperation<T, R> {
    data: T,
    receiver: R,
}

impl<T, R> OperationState for JustOperation<T, R>
where
    R: SetValue<Value = T>,
{
    fn start(self) {
        self.receiver.set_value(self.data);
    }
}

impl<T, R> Sender<R> for Just<T>
where
    R: SetValue<Value = T>,
{
    type Operation = JustOperation<T, R>;

    fn connect(self, receiver: R) -> Self::Operation {
        JustOperation {
            data: self.data,
            receiver,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use exec_test::receivers::ExpectReceiver;

    #[test]
    fn test_just() {
        let sender = Just::new(42);
        let operation = sender.connect(ExpectReceiver::new(42));
        operation.start();
    }
}
