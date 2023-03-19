use exec_core::receiver::{SetError, SetStopped, SetValue};
use exec_core::{OperationState, Sender};
use exec_executor::run_loop::RunLoop;
use std::cell::UnsafeCell;
use std::ptr::NonNull;

struct State<V, E> {
    value: UnsafeCell<Option<WaitResult<V, E>>>,
}

impl<V, E> State<V, E> {
    fn new() -> Self {
        Self {
            value: UnsafeCell::new(None),
        }
    }
}

#[derive(Debug)]
pub enum WaitResult<V, E> {
    Value(V),
    Error(E),
    Stopped,
}

pub struct SyncWaitReceiver<V, E> {
    state: NonNull<State<V, E>>,
    run_loop: NonNull<RunLoop>,
}

impl<V, E> SyncWaitReceiver<V, E> {
    fn new(state: &State<V, E>, run_loop: &RunLoop) -> Self {
        Self {
            state: NonNull::from(state),
            run_loop: NonNull::from(run_loop),
        }
    }
}

impl<V, E> SetValue for SyncWaitReceiver<V, E> {
    type Value = V;

    fn set_value(self, value: Self::Value) {
        unsafe {
            let _ = (*self.state.as_ref().value.get()).insert(WaitResult::Value(value));
            self.run_loop.as_ref().finish();
        }
    }
}

impl<V, E> SetError for SyncWaitReceiver<V, E> {
    type Error = E;

    fn set_error(self, error: Self::Error) {
        unsafe {
            let _ = (*self.state.as_ref().value.get()).insert(WaitResult::Error(error));
            self.run_loop.as_ref().finish();
        }
    }
}

impl<V, E> SetStopped for SyncWaitReceiver<V, E> {
    fn set_stopped(self) {
        unsafe {
            let _ = (*self.state.as_ref().value.get()).insert(WaitResult::Stopped);
            self.run_loop.as_ref().finish();
        }
    }
}

pub fn sync_wait<S, V, E>(sender: S) -> Result<Option<V>, E>
where
    S: Sender<SyncWaitReceiver<V, E>, Value = V, Error = E>,
{
    let run_loop = RunLoop::new();
    let mut state = State::new();

    // Launch the sender with a continuation that will fill in a variant
    // and notify a condition variable.
    let mut op = sender.connect(SyncWaitReceiver::new(&state, &run_loop));
    op.start();

    // Wait for the variant to be filled in.
    run_loop.run();

    match state.value.get_mut().take().unwrap() {
        WaitResult::Value(v) => Ok(Some(v)),
        WaitResult::Error(e) => Err(e),
        WaitResult::Stopped => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adaptors::then;
    use crate::factories::just;

    #[test]
    fn test_sync_wait() {
        let sender = just(42);
        let sender = then(sender, |v| v + 1);
        println!("{:?}", sync_wait(sender));
    }
}
