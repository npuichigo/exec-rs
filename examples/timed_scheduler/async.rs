//! Example demonstrating async/await with timed scheduler using coroutines.
//!
//! This example runs two concurrent async tasks:
//! - One prints "hello" 10 times, each delayed by 1 second
//! - Another prints "world" 5 times, each delayed by 3 seconds
//!
//! Uses async/await with loops. The async functions use `into_future()` to
//! convert exec-rs senders to Rust futures, and tokio::join! for concurrency.
//!
//! Note: `when_all` works with pure senders, but `from_future` (Future→Sender)
//! blocks during polling, so we use tokio::join! for async function concurrency.
//!
//! Usage: cargo run --example timed_hello_world_async --release

use exec_rs::future::SenderToFutureExt;
use exec_rs::schedulers::{TimedScheduler, TimedThreadContext, TimedThreadScheduler};
use std::time::{Duration, Instant};

async fn hello_loop(scheduler: TimedThreadScheduler, start: Instant) {
    for i in 1..=10 {
        let _ = scheduler
            .schedule_after(Duration::from_secs(1))
            .into_future()
            .await;
        println!("[{:5.1}s] hello {}", start.elapsed().as_secs_f64(), i);
    }
}

async fn world_loop(scheduler: TimedThreadScheduler, start: Instant) {
    for i in 1..=5 {
        let _ = scheduler
            .schedule_after(Duration::from_secs(3))
            .into_future()
            .await;
        println!("[{:5.1}s] world {}", start.elapsed().as_secs_f64(), i);
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    println!("Starting timed hello world example (async version)...");
    println!("  - 'hello' prints 10 times, every 1 second");
    println!("  - 'world' prints 5 times, every 3 seconds");
    println!();

    let start = Instant::now();

    // Create timed scheduler
    let timed_ctx = TimedThreadContext::new();
    let scheduler = timed_ctx.get_scheduler();

    // Run both loops concurrently using tokio::join!
    // (tokio provides cooperative scheduling for futures)
    tokio::join!(
        hello_loop(scheduler.clone(), start),
        world_loop(scheduler, start)
    );

    println!();
    println!("Completed in {:.1}s", start.elapsed().as_secs_f64());
}
