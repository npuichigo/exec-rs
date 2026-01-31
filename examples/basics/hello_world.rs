//! Hello World - Basic introduction to exec-rs
//!
//! This example demonstrates the fundamental concepts:
//! - Creating senders with `just` and `schedule`
//! - Transforming values with `then`
//! - Blocking on results with `sync_wait`
//!
//! Run with: cargo run --example hello_world

use exec_rs::consumers::sync_wait;
use exec_rs::factories::{just, schedule};
use exec_rs::schedulers::StaticThreadPool;
use exec_rs::traits::SenderExt;

fn main() {
    println!("=== Hello World Example ===\n");

    // Example 1: Simple value with just()
    println!("1. Simple just() sender:");
    let result = sync_wait(just(42)).unwrap();
    println!("   Result: {}\n", result);

    // Example 2: Transform with then()
    println!("2. Transform with then():");
    let result = sync_wait(just(21).then(|x| {
        println!("   Got: {}, doubling it...", x);
        x * 2
    }))
    .unwrap();
    println!("   Result: {}\n", result);

    // Example 3: Chain multiple transformations
    println!("3. Chain multiple then() calls:");
    let result = sync_wait(
        just(1)
            .then(|x| x + 1)
            .then(|x| x * 2)
            .then(|x| x + 3)
            .then(|x| format!("Final value: {}", x)),
    )
    .unwrap();
    println!("   {}\n", result);

    // Example 4: Schedule on thread pool
    println!("4. Schedule on thread pool:");
    let pool = StaticThreadPool::new(4);
    let scheduler = pool.get_scheduler();

    let result = sync_wait(schedule(scheduler).then(|_| {
        println!(
            "   Running on thread: {:?}",
            std::thread::current().name()
        );
        42
    }))
    .unwrap();
    println!("   Result: {}\n", result);

    // Example 5: Combine schedule and then
    println!("5. Full pipeline on thread pool:");
    let pool = StaticThreadPool::new(4);
    let scheduler = pool.get_scheduler();

    let result = sync_wait(
        schedule(scheduler)
            .then(|_| {
                println!("   Step 1: Starting computation");
                13
            })
            .then(|x| {
                println!("   Step 2: Got {}, adding 42", x);
                x + 42
            })
            .then(|x| {
                println!("   Step 3: Final value is {}", x);
                x
            }),
    )
    .unwrap();
    println!("   Result: {}", result);
}
