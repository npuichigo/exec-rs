use exec_core::receiver::SetValue;
use exec_core::{OperationState, Sender};
use scopeguard::defer;
use std::cell::UnsafeCell;
use std::ptr::NonNull;

pub fn submit<S, R>(sender: S, receiver: R)
where
    S: Sender<SubmitReceiver<R>>,
{
    let op = Box::leak(SubmitOperation::new(sender, receiver));
    op.op_state.as_mut().unwrap().start();
}

pub struct SubmitReceiver<R> {
    op_state: NonNull<SubmitOperationBase<R>>,
}

impl<R: SetValue> SetValue for SubmitReceiver<R> {
    type Value = R::Value;

    fn set_value(self, value: Self::Value) {
        unsafe {
            defer! {
                (self.op_state.as_ref().delete_fn)(self.op_state.as_ptr());
            }
            (*self.op_state.as_ref().receiver.get())
                .take()
                .unwrap()
                .set_value(value);
        }
    }
}

struct SubmitOperationBase<R> {
    receiver: UnsafeCell<Option<R>>,
    delete_fn: unsafe fn(*mut SubmitOperationBase<R>),
}

#[repr(C)]
struct SubmitOperation<S: Sender<SubmitReceiver<R>>, R> {
    base: SubmitOperationBase<R>,
    op_state: Option<S::Operation>,
}

impl<S, R> SubmitOperation<S, R>
where
    S: Sender<SubmitReceiver<R>>,
{
    fn new(sender: S, receiver: R) -> Box<Self> {
        let mut op = Box::new(Self {
            base: SubmitOperationBase {
                receiver: UnsafeCell::new(Some(receiver)),
                delete_fn: |op| {
                    unsafe {
                        let _ = Box::from_raw(op as *mut Self);
                    };
                },
            },
            op_state: None,
        });
        op.op_state = Some(sender.connect(SubmitReceiver {
            op_state: NonNull::from(&op.base),
        }));

        op
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::factories::just;
    use exec_test::receivers::ExpectReceiver;

    #[test]
    fn test_submit() {
        let just_sender = just(42);
        let receiver = ExpectReceiver::new(42);
        submit(just_sender, receiver);
    }
}
