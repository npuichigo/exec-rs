//! Split - Convert single-shot to multi-shot sender
//!
//! This example demonstrates using `split` to share a sender's result
//! with multiple consumers.
//!
//! Run with: cargo run --example split

use exec_rs::consumers::sync_wait;
use exec_rs::factories::just;
use exec_rs::traits::SenderExt;

fn main() {
    println!("=== Split Example ===\n");

    // Example 1: Basic split usage
    println!("1. Basic split - share result with multiple consumers:");

    // Create an "expensive" computation
    let expensive = just(()).then(|_| {
        println!("   Computing expensive value...");
        42
    });

    // Split it so we can use the result multiple times
    let shared = expensive.split();

    // Use the shared sender multiple times
    let result1 = sync_wait(shared.clone().then(|x| x * 2)).unwrap();
    let result2 = sync_wait(shared.clone().then(|x| x + 10)).unwrap();
    let result3 = sync_wait(shared.then(|x| format!("Value is {}", x))).unwrap();

    println!("   Result 1 (x*2): {}", result1);
    println!("   Result 2 (x+10): {}", result2);
    println!("   Result 3 (format): {}\n", result3);

    // Example 2: Split a computation pipeline
    println!("2. Split a computation pipeline:");

    let pipeline = just(10)
        .then(|x| {
            println!("   Step 1: {}", x);
            x * 2
        })
        .then(|x| {
            println!("   Step 2: {}", x);
            x + 5
        })
        .split();

    // The pipeline runs once, result shared
    let a = sync_wait(pipeline.clone()).unwrap();
    let b = sync_wait(pipeline.clone()).unwrap();
    let c = sync_wait(pipeline).unwrap();

    println!("   All three get same value: {}, {}, {}\n", a, b, c);

    // Example 3: Branch into different processing paths
    println!("3. Branch into different processing paths:");

    let data = just(vec![1, 2, 3, 4, 5]).split();

    let sum = sync_wait(data.clone().then(|v: Vec<i32>| v.iter().sum::<i32>())).unwrap();
    let product = sync_wait(data.clone().then(|v: Vec<i32>| v.iter().product::<i32>())).unwrap();
    let count = sync_wait(data.then(|v: Vec<i32>| v.len())).unwrap();

    println!("   Data: [1, 2, 3, 4, 5]");
    println!("   Sum: {}", sum);
    println!("   Product: {}", product);
    println!("   Count: {}\n", count);

    // Example 4: Lazy evaluation with split
    println!("4. Lazy evaluation - computation happens on first await:");

    let lazy = just(())
        .then(|_| {
            println!("   (computation executed)");
            "result"
        })
        .split();

    println!("   Created split sender, not executed yet...");
    println!("   First sync_wait:");
    let r1 = sync_wait(lazy.clone()).unwrap();
    println!("   Second sync_wait (uses cached result):");
    let r2 = sync_wait(lazy).unwrap();
    println!("   Results: {}, {}", r1, r2);
}
