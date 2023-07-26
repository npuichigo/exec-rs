use exec_core::receiver::SetError;
use exec_core::{OperationState, Sender};
use std::error::Error;

pub fn just_error<E: Error>(error: E) -> JustError<E> {
    JustError::new(error)
}

pub struct JustError<E> {
    error: E,
}

impl<E: Error> JustError<E> {
    pub fn new(error: E) -> Self {
        Self { error }
    }
}

pub struct JustOperation<E, R> {
    error: Option<E>,
    receiver: Option<R>,
}

impl<E, R> OperationState for JustOperation<E, R>
where
    R: SetError<Error = E>,
{
    fn start(&mut self) {
        if let (Some(receiver), Some(error)) = (self.receiver.take(), self.error.take()) {
            receiver.set_error(error);
        }
    }
}

impl<E, R> Sender<R> for JustError<E>
where
    R: SetError<Error = E>,
{
    type Value = ();
    type Error = R::Error;

    type Operation = JustOperation<E, R>;

    fn connect(self, receiver: R) -> Self::Operation {
        JustOperation {
            error: Some(self.error),
            receiver: Some(receiver),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use exec_test::errors::TestError;
    use exec_test::receivers::ExpectErrorReceiver;

    #[test]
    fn test_just_error() {
        let sender = JustError::new(TestError);
        let mut operation = sender.connect(ExpectErrorReceiver::new(TestError));
        operation.start();
    }
}
