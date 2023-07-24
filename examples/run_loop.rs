use exec::{start_detached, then};
use exec_core::Scheduler;
use exec_executor::run_loop::RunLoop;

fn main() {
    let run_loop = RunLoop::new();
    let mut scheduler = run_loop.get_scheduler();

    // Spawn tasks in run loop
    for i in 0..5 {
        start_detached(then(scheduler.schedule(), move |_| {
            println!("Run in loop with value: {}", i);
        }));
    }

    // Drain the works in run loop
    run_loop.finish();
    run_loop.run();
}
