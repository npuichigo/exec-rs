use super::operation_state::OperationState;

pub trait Sender<R> {
    type Operation: OperationState;

    fn connect(self, receiver: R) -> Self::Operation;
}
