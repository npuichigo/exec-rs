//! let_value - Dependent sender composition
//!
//! This example shows how to use `let_value` to chain senders where
//! the next sender depends on the result of the previous one.
//!
//! Unlike `then` which transforms a value, `let_value` allows you to
//! return a new sender that will be executed.
//!
//! Run with: cargo run --example let_value

use exec_rs::consumers::sync_wait;
use exec_rs::factories::just;
use exec_rs::traits::SenderExt;

fn main() {
    println!("=== let_value Example ===\n");

    // Example 1: Basic let_value
    println!("1. Basic let_value:");
    let result = sync_wait(just(21).let_value(|x| {
        println!("   Got {}, creating new sender with doubled value", x);
        just(x * 2)
    }))
    .unwrap();
    println!("   Result: {}\n", result);

    // Example 2: Chain multiple let_value calls
    println!("2. Chain multiple let_value:");
    let result = sync_wait(
        just(1)
            .let_value(|x| just(x + 1))
            .let_value(|x| just(x * 2))
            .let_value(|x| just(x + 10)),
    )
    .unwrap();
    println!("   1 -> +1 -> *2 -> +10 = {}\n", result);

    // Example 3: Mix then and let_value
    println!("3. Mix then and let_value:");
    let result = sync_wait(
        just(5)
            .then(|x| x * 2) // 10
            .let_value(|x| just(format!("Value is {}", x)))
            .then(|s| s.to_uppercase()),
    )
    .unwrap();
    println!("   Result: {}\n", result);

    // Example 4: Conditional sender selection
    println!("4. Conditional sender selection:");
    let use_fast_path = true;
    let result = sync_wait(just(42).let_value(move |x| {
        if use_fast_path {
            println!("   Taking fast path");
            just(x)
        } else {
            println!("   Taking slow path");
            just(x * 2)
        }
    }))
    .unwrap();
    println!("   Result: {}\n", result);

    // Example 5: Build complex pipelines
    println!("5. Complex pipeline:");
    let result = sync_wait(
        just(vec![1, 2, 3, 4, 5])
            .let_value(|v| {
                let sum: i32 = v.iter().sum();
                println!("   Sum of {:?} = {}", v, sum);
                just(sum)
            })
            .let_value(|sum| {
                let doubled = sum * 2;
                println!("   Doubled: {}", doubled);
                just(doubled)
            })
            .then(|x| format!("Final: {}", x)),
    )
    .unwrap();
    println!("   {}", result);
}
