use crate::consumers::submit;
use crate::consumers::submit::SubmitReceiver;
use exec_core::receiver::{SetError, SetValue};
use exec_core::Sender;
use std::error::Error;

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

impl<V, E: Error> SetError for StartDetachedReceiver<V, E> {
    type Error = E;

    fn set_error(self, error: Self::Error) {
        panic!(
            "StartDetachedReceiver::set_error called with error: {:?}",
            error
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::factories::{just, just_error};
    use exec_test::errors::TestError;

    #[test]
    fn test_start_detached() {
        let sender = just(1);
        start_detached(sender);
    }

    #[test]
    #[should_panic]
    fn test_start_detached_with_error() {
        let sender = just_error(TestError);
        start_detached(sender);
    }
}
