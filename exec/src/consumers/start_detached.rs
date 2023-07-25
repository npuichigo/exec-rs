use crate::consumers::submit;
use crate::consumers::submit::SubmitReceiver;
use exec_core::receiver::{SetError, SetValue};
use exec_core::Sender;

pub fn start_detached<S, V, E>(sender: S)
where
    S: Sender<SubmitReceiver<StartDetachedReceiver<V, E>>, Value = V, Error = E>,
{
    submit(
        sender,
        StartDetachedReceiver {
            _phantom: std::marker::PhantomData,
        },
    )
}

pub struct StartDetachedReceiver<V, E> {
    _phantom: std::marker::PhantomData<(V, E)>,
}

impl<V, E> SetValue for StartDetachedReceiver<V, E> {
    type Value = V;

    fn set_value(self, _value: Self::Value) {}
}

impl<V, E> SetError for StartDetachedReceiver<V, E> {
    type Error = E;

    fn set_error(self, _error: Self::Error) {
        panic!("StartDetachedReceiver::set_error");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::factories::{just, just_error};

    #[test]
    fn test_start_detached() {
        let sender = just(1);
        start_detached(sender);
    }

    #[test]
    #[should_panic]
    fn test_start_detached_with_error() {
        let sender = just_error("error");
        start_detached(sender);
    }
}
