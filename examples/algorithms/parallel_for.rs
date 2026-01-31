//! Parallel For - Execute a function for each index in parallel
//!
//! This example demonstrates using `bulk` for parallel iteration patterns.
//!
//! Run with: cargo run --example parallel_for

use exec_rs::consumers::sync_wait;
use exec_rs::factories::just;
use exec_rs::traits::SenderExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

fn main() {
    println!("=== Parallel For Example ===\n");

    // Example 1: Simple parallel for
    println!("1. Sum indices using bulk:");
    let result = sync_wait(just(0usize).bulk(10, |i, sum| {
        *sum += i;
    }))
    .unwrap();
    println!("   Sum of 0..10 = {}\n", result);

    // Example 2: Parallel accumulation with atomic
    println!("2. Parallel accumulation:");
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = counter.clone();

    sync_wait(just(()).bulk(100, move |i, _| {
        counter_clone.fetch_add(i, Ordering::SeqCst);
    }))
    .unwrap();
    println!("   Sum of 0..100 = {}\n", counter.load(Ordering::SeqCst));

    // Example 3: Collect results
    println!("3. Collect squared values:");
    let result = sync_wait(just(Vec::<usize>::new()).bulk(10, |i, vec| {
        vec.push(i * i);
    }))
    .unwrap();
    println!("   Squares: {:?}\n", result);

    // Example 4: Transform array elements
    println!("4. Transform array in place:");
    let data = vec![1, 2, 3, 4, 5];
    let result = sync_wait(just(data).bulk(5, |i, vec| {
        vec[i] *= 2;
    }))
    .unwrap();
    println!("   Doubled: {:?}\n", result);

    // Example 5: Parallel map-reduce pattern
    println!("5. Map-reduce pattern:");

    // Map phase: compute partial results
    let partial_sums = Arc::new(Mutex::new(vec![0usize; 4]));
    let partial_sums_clone = partial_sums.clone();

    sync_wait(just(()).bulk(100, move |i, _| {
        let bucket = i % 4;
        partial_sums_clone.lock().unwrap()[bucket] += i;
    }))
    .unwrap();

    // Reduce phase
    let total: usize = partial_sums.lock().unwrap().iter().sum();
    println!("   Partial sums: {:?}", *partial_sums.lock().unwrap());
    println!("   Total: {}\n", total);

    // Example 6: Matrix initialization
    println!("6. Initialize 2D matrix:");
    let rows = 3;
    let cols = 4;
    let result = sync_wait(just(vec![vec![0; cols]; rows]).bulk(rows, move |i, matrix| {
        for j in 0..cols {
            matrix[i][j] = i * cols + j;
        }
    }))
    .unwrap();
    println!("   Matrix:");
    for row in &result {
        println!("   {:?}", row);
    }
    println!();

    // Example 7: Chained bulk operations
    println!("7. Chained bulk operations:");
    let result = sync_wait(
        just(vec![0; 10])
            .bulk(10, |i, vec| {
                vec[i] = i + 1; // Initialize: [1, 2, 3, ..., 10]
            })
            .bulk(10, |i, vec| {
                vec[i] *= 2; // Double: [2, 4, 6, ..., 20]
            })
            .bulk(10, |i, vec| {
                vec[i] += i; // Add index: [2, 5, 8, ..., 29]
            }),
    )
    .unwrap();
    println!("   Result: {:?}", result);
}
