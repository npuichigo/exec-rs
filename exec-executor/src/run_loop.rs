use crate::utils::linked_list::{self, LinkedList};
use crate::Executable;
use exec_core::receiver::SetValue;
use exec_core::{OperationState, Scheduler, Sender};
use std::marker::{PhantomData, PhantomPinned};
use std::ptr::NonNull;
use std::sync::{Condvar, Mutex};

type TaskQueue = LinkedList<Task, <Task as linked_list::Link>::Target>;

pub struct Task {
    pointers: linked_list::Pointers<Task>,
    #[allow(dead_code)]
    operation: NonNull<dyn Executable>,
    _p: PhantomPinned,
}

generate_addr_of_methods! {
    impl<> Task<> {
        unsafe fn addr_of_pointers(self: NonNull<Self>) -> NonNull<linked_list::Pointers<Task>> {
            &self.pointers
        }
    }
}

pub struct Operation<R> {
    receiver: Option<R>,
    run_loop: NonNull<RunLoop>,
}

impl<R> Executable for Operation<R>
where
    R: SetValue<Value = ()>,
{
    fn execute(&mut self) {
        println!("execute {:p}", &self);
        if let Some(receiver) = self.receiver.take() {
            receiver.set_value(());
        }
    }
}

/// Storage for a task that is waiting to be executed.
///
/// We need to store the Operation along with the Task since there's no inheritance in Rust.
pub struct TaskOperation<R>(pub Task, pub Operation<R>);

impl<R> OperationState for TaskOperation<R> {
    fn start(&mut self) {
        println!("{:p}", &self.1);
        unsafe {
            self.1.run_loop.as_mut().push_front(NonNull::from(&self.0));
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
                println!("after {:p}", task.as_ptr());
                let task = task.as_mut();
                println!("after {:p}", task.operation.as_ptr());
                let operation = task.operation.as_mut();
                operation.execute();
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
    R: SetValue<Value = ()> + 'static,
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
    R: SetValue<Value = ()> + 'static,
{
    type Operation = TaskOperation<R>;

    fn connect(self, receiver: R) -> Self::Operation {
        let operation = Operation {
            receiver: Some(receiver),
            run_loop: self.run_loop,
        };
        println!("connect {:p}", &operation);
        let task = Task {
            pointers: linked_list::Pointers::new(),
            operation: NonNull::from(&operation),
            _p: PhantomPinned,
        };
        println!("connect {:p}", &task);
        TaskOperation(task, operation)
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
    use exec_test::receivers::ExpectReceiver;

    #[test]
    fn test_run_loop() {
        let run_loop = RunLoop::new();
        let mut scheduler = run_loop.get_scheduler();
        let sender = scheduler.schedule();
        let mut op = sender.connect(ExpectReceiver::new(()));
        println!("start {:p} {:p}", &op.0, &op.1);
        op.start();
        run_loop.finish();
        run_loop.run();
    }
}
