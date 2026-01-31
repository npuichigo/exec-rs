//! Retry - Retry patterns with senders
//!
//! This example demonstrates implementing retry logic patterns.
//! Since the type system is strict, we show practical retry approaches.
//!
//! Run with: cargo run --example retry

use exec_rs::consumers::sync_wait;
use exec_rs::factories::just;
use exec_rs::traits::SenderExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn main() {
    println!("=== Retry Example ===\n");

    // Example 1: Retry loop pattern
    println!("1. Retry loop (max 3 attempts):");
    let attempt_count = Arc::new(AtomicUsize::new(0));

    fn try_operation(attempt: &Arc<AtomicUsize>, succeed_at: usize) -> Result<String, String> {
        let n = attempt.fetch_add(1, Ordering::SeqCst) + 1;
        println!("   Attempt {}...", n);

        if n >= succeed_at {
            Ok(format!("Success on attempt {}", n))
        } else {
            Err(format!("Failed on attempt {}", n))
        }
    }

    let max_retries = 3;
    let mut result = try_operation(&attempt_count, 3);
    let mut retries = 0;

    while result.is_err() && retries < max_retries {
        retries += 1;
        result = try_operation(&attempt_count, 3);
    }

    match result {
        Ok(v) => println!("   Result: {}\n", v),
        Err(e) => println!("   Failed after {} retries: {}\n", max_retries, e),
    }

    // Example 2: Retry with sender
    println!("2. Retry with sender computation:");
    let counter = Arc::new(AtomicUsize::new(0));

    for i in 1..=3 {
        let counter_clone = counter.clone();
        let result = sync_wait(just(i).then(move |attempt| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
            println!("   Computing attempt {}...", attempt);
            attempt * 10
        }))
        .unwrap();

        if result >= 20 {
            println!("   Success with result: {}", result);
            break;
        } else {
            println!("   Result {} too low, retrying...", result);
        }
    }
    println!("   Total computations: {}\n", counter.load(Ordering::SeqCst));

    // Example 3: Exponential backoff simulation
    println!("3. Simulated exponential backoff:");
    let attempt = Arc::new(AtomicUsize::new(0));

    for i in 0..4 {
        let delay = 2u64.pow(i);
        println!("   Attempt {} (delay: {}ms)", i + 1, delay * 10);

        let n = attempt.fetch_add(1, Ordering::SeqCst);
        if n >= 2 {
            println!("   Success!");
            break;
        } else {
            println!("   Failed, backing off...");
            std::thread::sleep(std::time::Duration::from_millis(delay * 10));
        }
    }
    println!();

    // Example 4: Retry with fallback values
    println!("4. Retry with fallback:");
    fn fetch_data(source: &str) -> Option<i32> {
        println!("   Trying {}...", source);
        match source {
            "primary" => None,   // Simulate failure
            "secondary" => None, // Simulate failure
            "cache" => Some(42), // Fallback succeeds
            _ => None,
        }
    }

    let sources = ["primary", "secondary", "cache"];
    let mut result = None;

    for source in sources {
        if let Some(data) = fetch_data(source) {
            println!("   Got data from {}: {}", source, data);
            result = Some(data);
            break;
        }
    }

    match result {
        Some(v) => println!("   Final result: {}\n", v),
        None => println!("   All sources failed\n"),
    }

    // Example 5: Counting retry statistics
    println!("5. Track retry statistics:");
    let successes = Arc::new(AtomicUsize::new(0));
    let retries_needed = Arc::new(AtomicUsize::new(0));

    for i in 0..10 {
        let max_attempts = if i % 3 == 0 { 3 } else { 1 }; // Some need retries

        let mut succeeded = false;
        for attempt in 0..max_attempts {
            let result = sync_wait(just((i, attempt)).then(|(_task, att)| {
                // Succeed on last attempt
                att == max_attempts - 1
            }))
            .unwrap();

            if result {
                successes.fetch_add(1, Ordering::SeqCst);
                if attempt > 0 {
                    retries_needed.fetch_add(1, Ordering::SeqCst);
                }
                succeeded = true;
                break;
            }
        }

        if !succeeded {
            println!("   Task {} failed all attempts", i);
        }
    }

    println!(
        "   Successes: {}, Tasks needing retries: {}",
        successes.load(Ordering::SeqCst),
        retries_needed.load(Ordering::SeqCst)
    );
}
