use crate::utils::linked_list::{self, LinkedList};
use exec_core::receiver::SetValue;
use exec_core::{OperationState, Scheduler, Sender};
use std::marker::{PhantomData, PhantomPinned};
use std::ptr::NonNull;
use std::sync::{Condvar, Mutex};

type TaskQueue = LinkedList<Task, <Task as linked_list::Link>::Target>;

struct Task {
    pointers: linked_list::Pointers<Task>,
    execute: fn(*mut Task),
    _p: PhantomPinned,
}

generate_addr_of_methods! {
    impl<> Task<> {
        unsafe fn addr_of_pointers(self: NonNull<Self>) -> NonNull<linked_list::Pointers<Task>> {
            &self.pointers
        }
    }
}

#[repr(C)]
pub struct Operation<R> {
    base: Task,
    receiver: Option<R>,
    run_loop: NonNull<RunLoop>,
}

impl<R> Operation<R>
where
    R: SetValue<Value = ()>,
{
    fn execute(task: *mut Task) {
        let operation = unsafe { &mut *(task as *mut Operation<R>) };
        if let Some(receiver) = operation.receiver.take() {
            receiver.set_value(());
        }
    }
}

impl<R> OperationState for Operation<R> {
    fn start(&mut self) {
        unsafe {
            self.run_loop.as_mut().push_front(NonNull::from(&self.base));
        }
    }
}

pub struct RunLoop {
    inner: Mutex<Inner>,
    cv: Condvar,
}

struct Inner {
    queue: TaskQueue,
    stop: bool,
}

impl RunLoop {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                queue: TaskQueue::new(),
                stop: false,
            }),
            cv: Condvar::new(),
        }
    }

    fn push_front(&self, task: <Task as linked_list::Link>::Handle) {
        let mut inner = self.inner.lock().unwrap();
        inner.queue.push_front(task);
        self.cv.notify_one();
    }

    fn pop_back(&self) -> Option<<Task as linked_list::Link>::Handle> {
        let mut inner = self.inner.lock().unwrap();
        loop {
            let item = inner.queue.pop_back();
            if inner.stop || item.is_some() {
                break item;
            } else {
                inner = self.cv.wait(inner).unwrap();
            }
        }
    }

    pub fn finish(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.stop = true;
        self.cv.notify_all();
    }

    pub fn run(&self) {
        while let Some(mut task) = self.pop_back() {
            unsafe {
                (task.as_mut().execute)(task.as_ptr());
            }
        }
    }

    pub fn get_scheduler(&self) -> RunLoopScheduler {
        RunLoopScheduler {
            run_loop: NonNull::from(self),
        }
    }
}

#[derive(Copy, Clone)]
pub struct RunLoopScheduler {
    run_loop: NonNull<RunLoop>,
}

impl<R> Scheduler<R> for RunLoopScheduler
where
    R: SetValue<Value = ()>,
{
    type Sender = ScheduleTask<R>;

    fn schedule(&mut self) -> Self::Sender {
        ScheduleTask {
            run_loop: self.run_loop,
            _marker: PhantomData,
        }
    }
}

unsafe impl Send for RunLoopScheduler {}

/// Sender to schedule task in run loop.
pub struct ScheduleTask<R> {
    run_loop: NonNull<RunLoop>,
    _marker: PhantomData<R>,
}

impl<R> Sender<R> for ScheduleTask<R>
where
    R: SetValue<Value = ()>,
{
    type Value = R::Value;
    type Error = ();

    type Operation = Operation<R>;

    fn connect(self, receiver: R) -> Self::Operation {
        Operation {
            base: Task {
                pointers: linked_list::Pointers::new(),
                execute: Operation::<R>::execute,
                _p: PhantomPinned,
            },
            receiver: Some(receiver),
            run_loop: self.run_loop,
        }
    }
}

unsafe impl linked_list::Link for Task {
    type Handle = NonNull<Task>;
    type Target = Task;

    fn as_raw(handle: &NonNull<Task>) -> NonNull<Task> {
        *handle
    }

    unsafe fn from_raw(ptr: NonNull<Task>) -> NonNull<Task> {
        ptr
    }

    unsafe fn pointers(target: NonNull<Task>) -> NonNull<linked_list::Pointers<Task>> {
        Task::addr_of_pointers(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use exec_test::receivers::ExpectValueReceiver;

    #[test]
    fn test_run_loop() {
        let run_loop = RunLoop::new();
        let mut scheduler = run_loop.get_scheduler();
        let sender = scheduler.schedule();
        let mut op = sender.connect(ExpectValueReceiver::new(()));
        // Schedule the work on run loop
        op.start();
        run_loop.finish();
        // Drain the works in run loop
        run_loop.run();
    }
}
