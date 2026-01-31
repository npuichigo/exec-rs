//! Error Handling - Working with errors in sender chains
//!
//! This example demonstrates:
//! - Creating error senders with `just_error`
//! - Handling errors at the sync_wait level
//! - Understanding error types in sender chains
//!
//! Run with: cargo run --example error_handling

use exec_rs::consumers::{sync_wait, SyncWaitError};
use exec_rs::factories::{just, just_error, just_stopped};
use exec_rs::traits::SenderExt;

fn main() {
    println!("=== Error Handling Example ===\n");

    // Example 1: Basic error sender
    println!("1. Basic error sender:");
    let result: Result<(), SyncWaitError<&str>> = sync_wait(just_error("something went wrong"));
    match result {
        Ok(_) => println!("   Success (unexpected)"),
        Err(SyncWaitError::Error(e)) => println!("   Got error: {}", e),
        Err(SyncWaitError::Stopped) => println!("   Stopped (unexpected)"),
    }
    println!();

    // Example 2: Successful path with then
    println!("2. Successful path (no error):");
    let result = sync_wait(
        just(10)
            .then(|x| {
                println!("   Step 1: got {}", x);
                x * 2
            })
            .then(|x| {
                println!("   Step 2: got {}", x);
                x + 5
            })
            .then(|x| {
                println!("   Step 3: got {}", x);
                x
            }),
    )
    .unwrap();
    println!("   Final result: {}\n", result);

    // Example 3: Error with integer type
    println!("3. Error with integer type:");
    let result: Result<(), SyncWaitError<i32>> = sync_wait(just_error(404));
    match result {
        Ok(()) => println!("   Value received (unexpected)"),
        Err(SyncWaitError::Error(code)) => println!("   Error code: {}", code),
        Err(SyncWaitError::Stopped) => println!("   Stopped"),
    }
    println!();

    // Example 4: Different error types
    println!("4. Working with custom error types:");
    #[derive(Debug)]
    struct CustomError(String);

    let result: Result<(), SyncWaitError<CustomError>> =
        sync_wait(just_error(CustomError("custom error occurred".to_string())));

    match result {
        Ok(_) => println!("   Value received"),
        Err(SyncWaitError::Error(e)) => println!("   Custom error: {:?}", e),
        Err(SyncWaitError::Stopped) => println!("   Stopped"),
    }
    println!();

    // Example 5: Stopped signal
    println!("5. Stopped signal:");
    let result: Result<(), SyncWaitError<std::convert::Infallible>> = sync_wait(just_stopped());
    match result {
        Ok(_) => println!("   Value received"),
        Err(SyncWaitError::Error(_)) => println!("   Error"),
        Err(SyncWaitError::Stopped) => println!("   Operation was stopped"),
    }
    println!();

    // Example 6: Handle Result in then
    println!("6. Using Result in then for error simulation:");
    let result = sync_wait(just(5).then(|x| {
        if x > 0 {
            Ok(x * 2)
        } else {
            Err("negative input")
        }
    }))
    .unwrap();
    println!("   Result: {:?}\n", result);

    // Example 7: Materialize completions
    println!("7. Materialize completions:");
    use exec_rs::adaptors::Completion;

    let value_result = sync_wait(just(42).materialize()).unwrap();
    println!("   Value materialized: {:?}", value_result);

    let stopped_result = sync_wait(just_stopped().materialize()).unwrap();
    println!("   Stopped materialized: {:?}", stopped_result);

    match value_result {
        Completion::Value(v) => println!("   Extracted value: {}", v),
        Completion::Error(_) => println!("   Got error"),
        Completion::Stopped => println!("   Got stopped"),
    }
}
