use exec::{start_detached, then};
use exec_core::Scheduler;
use exec_executor::SingleThreadContext;
use std::thread;

fn main() {
    let context = SingleThreadContext::new();
    let mut scheduler = context.get_scheduler();

    println!("Main run in thread: {:?}", thread::current().id());

    for i in 0..5 {
        start_detached(then(scheduler.schedule(), move |_| {
            println!(
                "Run in thread {:?} loop with value: {}",
                thread::current().id(),
                i
            );
        }));
    }
}
