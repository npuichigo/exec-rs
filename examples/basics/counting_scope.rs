//! Counting Scope - Structured concurrency with scopes
//!
//! This example demonstrates using CountingScope for structured concurrency,
//! where you can spawn work and wait for all spawned work to complete.
//!
//! Run with: cargo run --example counting_scope

use exec_rs::consumers::sync_wait;
use exec_rs::factories::just;
use exec_rs::scope::CountingScope;
use exec_rs::traits::SenderExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn main() {
    println!("=== Counting Scope Example ===\n");

    // Example 1: Basic scope usage
    println!("1. Basic scope - spawn and wait:");
    let scope = CountingScope::new();
    let counter = Arc::new(AtomicUsize::new(0));

    // Spawn some work
    for i in 0..5 {
        let counter_clone = counter.clone();
        scope.spawn(just(i).then(move |x| {
            println!("   Task {} running", x);
            counter_clone.fetch_add(1, Ordering::SeqCst);
        }));
    }

    // Wait for all work to complete
    sync_wait(scope.when_empty()).unwrap();
    println!(
        "   All tasks complete. Counter: {}\n",
        counter.load(Ordering::SeqCst)
    );

    // Example 2: Collecting results
    println!("2. Collect results from spawned tasks:");
    let scope = CountingScope::new();
    let results = Arc::new(std::sync::Mutex::new(Vec::new()));

    // Spawn computations
    for i in 1..=5 {
        let results_clone = results.clone();
        scope.spawn(just(i).then(move |n| {
            let sum: i32 = (1..=n).sum();
            println!("   Sum(1..={}) = {}", n, sum);
            results_clone.lock().unwrap().push(sum);
        }));
    }

    sync_wait(scope.when_empty()).unwrap();
    let collected: Vec<i32> = results.lock().unwrap().clone();
    let total: i32 = collected.iter().sum();
    println!("   Partial sums: {:?}", collected);
    println!("   Grand total: {}\n", total);

    // Example 3: Check scope state
    println!("3. Check scope state:");
    let scope = CountingScope::new();
    println!("   Initially empty: {}", scope.active_count() == 0);

    scope.spawn(just(()).then(|_| {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }));
    println!("   After spawn (active count): {}", scope.active_count());

    sync_wait(scope.when_empty()).unwrap();
    println!("   After wait: {}\n", scope.active_count() == 0);

    // Example 4: Producer pattern
    println!("4. Producer pattern:");
    let scope = CountingScope::new();
    let items = Arc::new(std::sync::Mutex::new(Vec::new()));

    // Producer: spawn items
    for i in 0..10 {
        let items_clone = items.clone();
        scope.spawn(just(i).then(move |x| {
            items_clone.lock().unwrap().push(x * x);
        }));
    }

    // Wait for all producers
    sync_wait(scope.when_empty()).unwrap();

    // Process all items
    let result: i32 = items.lock().unwrap().iter().sum();
    println!("   Produced squares: {:?}", *items.lock().unwrap());
    println!("   Sum of squares: {}\n", result);

    // Example 5: Nested scopes
    println!("5. Sequential scope usage:");
    for batch in 0..3 {
        let scope = CountingScope::new();
        let batch_sum = Arc::new(AtomicUsize::new(0));

        for i in 0..3 {
            let sum_clone = batch_sum.clone();
            scope.spawn(just((batch, i)).then(move |(b, idx)| {
                let value = b * 10 + idx;
                sum_clone.fetch_add(value, Ordering::SeqCst);
            }));
        }

        sync_wait(scope.when_empty()).unwrap();
        println!("   Batch {} sum: {}", batch, batch_sum.load(Ordering::SeqCst));
    }
}
