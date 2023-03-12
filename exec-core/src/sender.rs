use super::operation_state::OperationState;

pub trait Sender<R> {
    type Output;
    type Operation: OperationState;

    fn connect(self, receiver: R) -> Self::Operation;
}
