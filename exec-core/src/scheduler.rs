use crate::Sender;

pub trait Scheduler<R>: Send + Clone {
    type Sender: Sender<R>;

    fn schedule(&mut self) -> Self::Sender;
}
