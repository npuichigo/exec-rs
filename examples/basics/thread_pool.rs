//! Thread Pool - Running work on a thread pool
//!
//! This example demonstrates using StaticThreadPool for parallel execution.
//!
//! Run with: cargo run --example thread_pool

use exec_rs::adaptors::{on, when_all};
use exec_rs::consumers::sync_wait;
use exec_rs::factories::{just, schedule};
use exec_rs::schedulers::StaticThreadPool;
use exec_rs::traits::SenderExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn main() {
    println!("=== Thread Pool Example ===\n");

    // Example 1: Basic thread pool usage
    println!("1. Basic thread pool:");
    let pool = StaticThreadPool::new(4);
    let scheduler = pool.get_scheduler();
    println!("   Created pool with {} threads", pool.thread_count());

    let result = sync_wait(on(scheduler.clone(), just(42))).unwrap();
    println!("   Result: {}\n", result);

    // Example 2: Schedule work on pool
    println!("2. Schedule and transform:");
    let result = sync_wait(
        schedule(scheduler.clone())
            .then(|_| {
                let name = thread::current()
                    .name()
                    .unwrap_or("unnamed")
                    .to_string();
                println!("   Running on: {}", name);
                42
            })
            .then(|x| x * 2),
    )
    .unwrap();
    println!("   Result: {}\n", result);

    // Example 3: Multiple tasks on pool
    println!("3. Multiple independent tasks:");
    let counter = Arc::new(AtomicUsize::new(0));

    for i in 0..10 {
        let counter_clone = counter.clone();
        let scheduler_clone = scheduler.clone();
        sync_wait(on(
            scheduler_clone,
            just(i).then(move |x| {
                counter_clone.fetch_add(x, Ordering::SeqCst);
            }),
        ))
        .unwrap();
    }
    println!("   Sum of 0..10: {}\n", counter.load(Ordering::SeqCst));

    // Example 4: Parallel work with when_all (2 tasks)
    println!("4. Parallel work with when_all:");
    let result = sync_wait(when_all(
        on(scheduler.clone(), just("task1").then(|s| {
            thread::sleep(Duration::from_millis(10));
            format!("{} done", s)
        })),
        on(scheduler.clone(), just("task2").then(|s| {
            thread::sleep(Duration::from_millis(10));
            format!("{} done", s)
        })),
    ))
    .unwrap();
    println!("   Results: {:?}\n", result);

    // Example 5: Bulk operation on thread pool
    println!("5. Bulk operation:");
    let sum = Arc::new(AtomicUsize::new(0));
    let sum_clone = sum.clone();

    let result = sync_wait(
        on(
            scheduler.clone(),
            just(0usize).bulk(10, move |i, _| {
                sum_clone.fetch_add(i, Ordering::SeqCst);
            }),
        )
        .then(move |_| sum.load(Ordering::SeqCst)),
    )
    .unwrap();
    println!("   Sum of indices 0..10: {}\n", result);

    // Example 6: CPU-bound work (multiple sequential when_all pairs)
    println!("6. CPU-bound work:");
    fn compute_heavy(n: u64) -> u64 {
        (1..=n).map(|x| x * x).sum()
    }

    // Run first pair
    let result1 = sync_wait(when_all(
        on(scheduler.clone(), just(10_000u64).then(compute_heavy)),
        on(scheduler.clone(), just(20_000u64).then(compute_heavy)),
    ))
    .unwrap();

    // Run second pair
    let result2 = sync_wait(when_all(
        on(scheduler.clone(), just(30_000u64).then(compute_heavy)),
        on(scheduler.clone(), just(40_000u64).then(compute_heavy)),
    ))
    .unwrap();

    println!("   Sum of squares up to 10k: {}", result1.0);
    println!("   Sum of squares up to 20k: {}", result1.1);
    println!("   Sum of squares up to 30k: {}", result2.0);
    println!("   Sum of squares up to 40k: {}", result2.1);
}
