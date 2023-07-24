use crate::run_loop::RunLoopScheduler;
use crate::RunLoop;
use std::sync::Arc;
use std::thread;

pub struct SingleThreadContext {
    thread: Option<thread::JoinHandle<()>>,
    run_loop: Arc<RunLoop>,
}

impl SingleThreadContext {
    pub fn new() -> Self {
        let run_loop = Arc::new(RunLoop::new());
        let thread;
        {
            let run_loop = run_loop.clone();
            thread = thread::spawn(move || {
                run_loop.clone().run();
            });
        }
        Self {
            thread: Some(thread),
            run_loop,
        }
    }

    pub fn get_scheduler(&self) -> RunLoopScheduler {
        self.run_loop.get_scheduler()
    }
}

impl Default for SingleThreadContext {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for SingleThreadContext {
    fn drop(&mut self) {
        self.run_loop.finish();
        self.thread.take().unwrap().join().unwrap();
    }
}
