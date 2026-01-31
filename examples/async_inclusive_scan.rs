//! Parallel Inclusive Scan Example from P2300 Proposal
//!
//! This example demonstrates the async_inclusive_scan algorithm from the
//! std::execution proposal (P2300). It shows how to use bulk operations
//! to parallelize a scan algorithm across multiple tiles.
//!
//! The algorithm works in three phases:
//! 1. Each tile performs a local inclusive scan, storing the final value in partials
//! 2. Perform an inclusive scan on the partials themselves
//! 3. Add the scanned partial to each element in each tile

use exec_rs::consumers::sync_wait;
use exec_rs::factories::just;
use exec_rs::schedulers::{StaticThreadPool, ThreadPoolScheduler};
use exec_rs::traits::SenderExt;
use std::cmp::min;

/// Performs an inclusive scan on a slice, returning the last element.
/// Equivalent to C++ std::inclusive_scan.
fn inclusive_scan(input: &[f64], output: &mut [f64]) -> f64 {
    if input.is_empty() {
        return 0.0;
    }

    let mut sum = input[0];
    output[0] = sum;

    for i in 1..input.len() {
        sum += input[i];
        output[i] = sum;
    }

    sum
}

/// Performs an in-place inclusive scan on a vector.
fn inclusive_scan_inplace(data: &mut [f64]) {
    if data.is_empty() {
        return;
    }

    for i in 1..data.len() {
        data[i] += data[i - 1];
    }
}

/// Async inclusive scan using sender/receiver pattern.
///
/// This implementation follows the P2300 proposal example:
/// 1. Divide the input into tiles
/// 2. Parallel phase: Each tile performs local inclusive scan
/// 3. Sequential phase: Scan the partial results
/// 4. Parallel phase: Add partial sums to each tile
///
/// Note: In this simplified version, we don't use continues_on for scheduler
/// transfer, focusing instead on demonstrating the bulk algorithm pattern.
fn async_inclusive_scan(
    _sch: ThreadPoolScheduler,
    input: &[f64],
    output: &mut [f64],
    init: f64,
    tile_count: usize,
) {
    let input_len = input.len();
    let tile_size = (input_len + tile_count - 1) / tile_count;

    // Create partials vector with init value at position 0
    let mut partials = vec![0.0; tile_count + 1];
    partials[0] = init;

    // Copy input/output data for use in closures
    let input_vec: Vec<f64> = input.to_vec();
    let output_vec: Vec<f64> = output.to_vec();

    // Use a shared mutable state for the output
    let output_shared = std::sync::Arc::new(std::sync::Mutex::new(output_vec));
    let output_shared_clone = output_shared.clone();
    let output_shared_final = output_shared.clone();

    // Build the sender pipeline using fluent API (method chaining)
    // This mirrors the C++ example with pipe operator:
    //   just(partials)
    //       | bulk(tile_count, ...)  // Phase 1: Local scans
    //       | then(...)               // Phase 2: Scan partials
    //       | bulk(tile_count, ...)  // Phase 3: Add partial sums
    //       | then(...)               // Return result

    let result = just(partials)
        // Phase 1: Each tile performs local inclusive scan
        .bulk(tile_count, {
            let input_vec = input_vec.clone();
            move |i: usize, partials: &mut Vec<f64>| {
                let start = i * tile_size;
                let end = min(input_len, (i + 1) * tile_size);

                if start < end {
                    let mut output_guard = output_shared.lock().unwrap();

                    // Perform inclusive scan on this tile
                    let last_value =
                        inclusive_scan(&input_vec[start..end], &mut output_guard[start..end]);

                    // Store the last value in partials[i + 1]
                    partials[i + 1] = last_value;
                }
            }
        })
        // Phase 2: Scan the partials
        .then(|mut partials: Vec<f64>| {
            inclusive_scan_inplace(&mut partials);
            partials
        })
        // Phase 3: Add partial sums to each tile
        .bulk(tile_count, {
            move |i: usize, partials: &mut Vec<f64>| {
                let start = i * tile_size;
                let end = min(input_len, (i + 1) * tile_size);

                if start < end {
                    let partial_sum = partials[i];
                    let mut output_guard = output_shared_clone.lock().unwrap();

                    for j in start..end {
                        output_guard[j] += partial_sum;
                    }
                }
            }
        })
        // Return the output
        .then(move |_partials: Vec<f64>| {
            let output_guard = output_shared_final.lock().unwrap();
            output_guard.clone()
        });

    let result = sync_wait(result).unwrap();

    // Copy result back to output
    output.copy_from_slice(&result);
}

/// Sequential inclusive scan for verification.
fn sequential_inclusive_scan(input: &[f64], output: &mut [f64], init: f64) {
    if input.is_empty() {
        return;
    }

    let mut sum = init + input[0];
    output[0] = sum;

    for i in 1..input.len() {
        sum += input[i];
        output[i] = sum;
    }
}

fn main() {
    println!("=== Async Inclusive Scan Example (P2300) ===\n");

    // Create a thread pool with 4 threads
    let pool = StaticThreadPool::new(4);
    let scheduler = pool.get_scheduler();

    // Test with a small array
    let input: Vec<f64> = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
    let mut output_async = vec![0.0; input.len()];
    let mut output_seq = vec![0.0; input.len()];
    let init = 0.0;
    let tile_count = 4;

    println!("Input: {:?}", input);
    println!("Initial value: {}", init);
    println!("Tile count: {}", tile_count);
    println!();

    // Run sequential scan for reference
    sequential_inclusive_scan(&input, &mut output_seq, init);
    println!("Sequential result: {:?}", output_seq);

    // Run async scan
    async_inclusive_scan(scheduler.clone(), &input, &mut output_async, init, tile_count);
    println!("Async result:      {:?}", output_async);

    // Verify results match
    let tolerance = 1e-10;
    let results_match = output_async
        .iter()
        .zip(output_seq.iter())
        .all(|(a, b)| (a - b).abs() < tolerance);

    println!("\nResults match: {}", results_match);

    // Test with a larger array
    println!("\n--- Larger Test ---");
    let large_input: Vec<f64> = (1..=100).map(|x| x as f64).collect();
    let mut large_output_async = vec![0.0; large_input.len()];
    let mut large_output_seq = vec![0.0; large_input.len()];

    sequential_inclusive_scan(&large_input, &mut large_output_seq, 0.0);
    async_inclusive_scan(
        scheduler,
        &large_input,
        &mut large_output_async,
        0.0,
        8, // 8 tiles for 100 elements
    );

    let large_results_match = large_output_async
        .iter()
        .zip(large_output_seq.iter())
        .all(|(a, b)| (a - b).abs() < tolerance);

    println!("Input size: {}", large_input.len());
    println!("Expected final value: {}", large_output_seq.last().unwrap());
    println!("Async final value:    {}", large_output_async.last().unwrap());
    println!("Results match: {}", large_results_match);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inclusive_scan_basic() {
        let input = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let mut output = vec![0.0; 5];
        inclusive_scan(&input, &mut output);
        assert_eq!(output, vec![1.0, 3.0, 6.0, 10.0, 15.0]);
    }

    #[test]
    fn test_inclusive_scan_inplace() {
        let mut data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        inclusive_scan_inplace(&mut data);
        assert_eq!(data, vec![1.0, 3.0, 6.0, 10.0, 15.0]);
    }

    #[test]
    fn test_async_inclusive_scan() {
        let pool = StaticThreadPool::new(4);
        let scheduler = pool.get_scheduler();

        let input: Vec<f64> = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let mut output_async = vec![0.0; input.len()];
        let mut output_seq = vec![0.0; input.len()];

        sequential_inclusive_scan(&input, &mut output_seq, 0.0);
        async_inclusive_scan(scheduler, &input, &mut output_async, 0.0, 4);

        let tolerance = 1e-10;
        for (a, b) in output_async.iter().zip(output_seq.iter()) {
            assert!((a - b).abs() < tolerance, "Mismatch: {} vs {}", a, b);
        }
    }

    #[test]
    fn test_async_inclusive_scan_with_init() {
        let pool = StaticThreadPool::new(4);
        let scheduler = pool.get_scheduler();

        let input: Vec<f64> = vec![1.0, 2.0, 3.0, 4.0];
        let mut output_async = vec![0.0; input.len()];
        let mut output_seq = vec![0.0; input.len()];

        sequential_inclusive_scan(&input, &mut output_seq, 10.0);
        async_inclusive_scan(scheduler, &input, &mut output_async, 10.0, 2);

        let tolerance = 1e-10;
        for (a, b) in output_async.iter().zip(output_seq.iter()) {
            assert!((a - b).abs() < tolerance, "Mismatch: {} vs {}", a, b);
        }
    }

    #[test]
    fn test_async_inclusive_scan_large() {
        let pool = StaticThreadPool::new(4);
        let scheduler = pool.get_scheduler();

        let input: Vec<f64> = (1..=1000).map(|x| x as f64).collect();
        let mut output_async = vec![0.0; input.len()];
        let mut output_seq = vec![0.0; input.len()];

        sequential_inclusive_scan(&input, &mut output_seq, 0.0);
        async_inclusive_scan(scheduler, &input, &mut output_async, 0.0, 16);

        let tolerance = 1e-10;
        for (i, (a, b)) in output_async.iter().zip(output_seq.iter()).enumerate() {
            assert!(
                (a - b).abs() < tolerance,
                "Mismatch at index {}: {} vs {}",
                i,
                a,
                b
            );
        }
    }
}
