//! when_all - Concurrent composition
//!
//! This example demonstrates running multiple senders concurrently
//! and collecting all their results.
//!
//! Run with: cargo run --example when_all

use exec_rs::adaptors::when_all;
use exec_rs::consumers::sync_wait;
use exec_rs::factories::just;

fn main() {
    println!("=== when_all Example ===\n");

    // Example 1: Basic when_all with two senders
    println!("1. Basic when_all (2 senders):");
    let result = sync_wait(when_all(just(1), just(2))).unwrap();
    println!("   Results: {:?}\n", result);

    // Example 2: Different types
    println!("2. Different types:");
    let result = sync_wait(when_all(just(42), just("hello"))).unwrap();
    println!("   Results: ({}, {})\n", result.0, result.1);

    // Example 3: Compute multiple values
    println!("3. Parallel computations:");
    let sum1: i32 = (1..=1000).sum();
    let sum2: i32 = (1..=100).map(|x| x * x).sum();

    let result = sync_wait(when_all(just(sum1), just(sum2))).unwrap();
    println!("   Sum 1..1000: {}", result.0);
    println!("   Sum of squares 1..100: {}", result.1);
    println!("   Total: {}\n", result.0 + result.1);

    // Example 4: With complex values
    println!("4. With vectors:");
    let result = sync_wait(when_all(just(vec![1, 2, 3]), just(vec![4, 5, 6]))).unwrap();
    println!("   First: {:?}", result.0);
    println!("   Second: {:?}", result.1);
    let combined: Vec<_> = result.0.into_iter().chain(result.1).collect();
    println!("   Combined: {:?}\n", combined);

    // Example 5: Multiple pairs sequentially
    println!("5. Multiple when_all pairs:");
    let result1 = sync_wait(when_all(just(1), just(2))).unwrap();
    let result2 = sync_wait(when_all(just(3), just(4))).unwrap();
    println!("   First pair: {:?}", result1);
    println!("   Second pair: {:?}", result2);
    let total = result1.0 + result1.1 + result2.0 + result2.1;
    println!("   Sum: {}\n", total);

    // Example 6: With string computations
    println!("6. String computations:");
    let result = sync_wait(when_all(
        just("Hello".to_string()),
        just(" World!".to_string()),
    ))
    .unwrap();
    println!("   Concatenated: {}{}", result.0, result.1);
}
