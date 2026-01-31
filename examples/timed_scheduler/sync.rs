//! Example demonstrating when_all with timed scheduler.
//!
//! This example runs two concurrent senders:
//! - One prints "hello" 10 times, each delayed by 1 second
//! - Another prints "world" 5 times, each delayed by 3 seconds
//!
//! Usage: cargo run --example timed_hello_world --release

use exec_rs::adaptors::{repeat_n, then, when_all};
use exec_rs::consumers::sync_wait;
use exec_rs::schedulers::{TimedScheduler, TimedThreadContext};
use std::time::{Duration, Instant};

fn main() {
    println!("Starting timed hello world example...");
    println!("  - 'hello' prints 10 times, every 1 second");
    println!("  - 'world' prints 5 times, every 3 seconds");
    println!();

    let start = Instant::now();

    // Create timed scheduler
    let timed_ctx = TimedThreadContext::new();
    let s = timed_ctx.get_scheduler();

    // Build hello sender: 10 prints, 1 second apart
    let hello_sender = {
        let s = s.clone();
        repeat_n(
            move |i| {
                let s = s.clone();
                let st = start;
                then(s.schedule_after(Duration::from_secs(1)), move |_| {
                    println!("[{:5.1}s] hello {}", st.elapsed().as_secs_f64(), i + 1)
                })
            },
            10,
        )
    };

    // Build world sender: 5 prints, 3 seconds apart
    let world_sender = {
        let s = s.clone();
        repeat_n(
            move |i| {
                let s = s.clone();
                let st = start;
                then(s.schedule_after(Duration::from_secs(3)), move |_| {
                    println!("[{:5.1}s] world {}", st.elapsed().as_secs_f64(), i + 1)
                })
            },
            5,
        )
    };

    // Run both concurrently with when_all
    let combined = when_all(hello_sender, world_sender);

    // Execute and wait for completion
    let result = sync_wait(combined);

    match result {
        Ok(_) => {
            println!();
            println!("Completed in {:.1}s", start.elapsed().as_secs_f64());
        }
        Err(e) => {
            eprintln!("Error: {:?}", e);
        }
    }
}
