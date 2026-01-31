# exec-rs

A Rust implementation of C++ std::execution (P2300) based on the [NVIDIA/stdexec](https://github.com/NVIDIA/stdexec) reference implementation.

## Overview

exec-rs provides lazy, composable asynchronous programming primitives with zero-cost abstractions using the **sender/receiver** model from C++ P2300.

```rust
use exec_rs::prelude::*;

// Compose async operations
let result = sync_wait(
    just(21)
        .then(|x| x * 2)
        .let_value(|x| just(format!("Result: {}", x)))
).unwrap();

assert_eq!(result, "Result: 42");
```

## Features

- **Zero-cost abstractions**: No heap allocation for basic operations
- **Type-safe composition**: Compile-time checked sender pipelines
- **Structured concurrency**: Scopes ensure proper lifetime management
- **Cancellation support**: Stop tokens propagate through pipelines
- **Multiple schedulers**: Thread pools, timed schedulers, inline execution
- **Async I/O**: mio-based networking with sender operations (P2762)

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
exec-rs = "0.1"
```

### Basic Example

```rust
use exec_rs::consumers::sync_wait;
use exec_rs::factories::just;
use exec_rs::traits::SenderExt;

fn main() {
    // Simple value transformation
    let result = sync_wait(
        just(1)
            .then(|x| x + 1)
            .then(|x| x * 2)
    ).unwrap();

    println!("Result: {}", result); // Result: 4
}
```

### Thread Pool Example

```rust
use exec_rs::adaptors::on;
use exec_rs::consumers::sync_wait;
use exec_rs::factories::just;
use exec_rs::schedulers::StaticThreadPool;
use exec_rs::traits::SenderExt;

fn main() {
    let pool = StaticThreadPool::new(4);
    let scheduler = pool.get_scheduler();

    let result = sync_wait(on(scheduler, just(42).then(|x| x * 2))).unwrap();
    println!("Result: {}", result); // Result: 84
}
```

## Project Statistics

- **Source Files**: 86 Rust files
- **Unit Tests**: 272 passing
- **Examples**: 14 examples across basics, algorithms, networking, and timed scheduling

## Examples

Run examples with `cargo run --example <name>`:

### Basics
| Example | Description |
|---------|-------------|
| `hello_world` | Basic `just`, `then`, `schedule`, `sync_wait` |
| `let_value` | Dependent sender composition |
| `when_all` | Concurrent composition |
| `error_handling` | Error types, `just_error`, `materialize` |
| `thread_pool` | `StaticThreadPool`, `on()`, parallel work |
| `split` | Multi-shot sender with `split` |
| `counting_scope` | Structured concurrency |

### Algorithms
| Example | Description |
|---------|-------------|
| `parallel_for` | Bulk iteration patterns |
| `retry` | Retry patterns with senders |
| `async_inclusive_scan` | Parallel prefix sum algorithm |

### Networking
| Example | Description |
|---------|-------------|
| `tokio_echo` | Tokio-based echo server (baseline) |
| `exec_echo` | exec-rs sender-based echo server |

### Timed Scheduler
| Example | Description |
|---------|-------------|
| `timed_scheduler` | Synchronous timed operations |
| `timed_scheduler_async` | Async timed operations |

## Implementation Status

### Core Components

| Phase | Description | Status |
|-------|-------------|--------|
| 1 | Core Traits + Stop Tokens | ✅ Complete |
| 2 | Basic Senders (Factories) | ✅ Complete |
| 3 | sync_wait + RunLoop | ✅ Complete |
| 4 | Value Transformation Adaptors | ✅ Complete |
| 5 | Sender Chaining (let_*) | ✅ Complete |
| 6 | Execution Context Switching | ✅ Complete |
| 7 | Concurrent Composition | ✅ Complete |
| 8 | Additional Adaptors | ✅ Complete |
| 9 | Structured Concurrency | ✅ Complete |
| 10 | Sequence Senders | ⚠️ Partial |
| 11 | Type-Erased Senders | ✅ Complete |
| 12 | Advanced Schedulers | ✅ Complete |
| 13 | Coroutine Integration | ✅ Complete |
| 14 | Utilities | ✅ Complete |
| 15 | Future Bridge | ✅ Complete |
| 16 | Networking (P2762) | ✅ Complete |

### Sender Factories (`src/factories/`)

- `just(value)` - Immediate value completion
- `just_error(error)` - Immediate error completion
- `just_stopped()` - Immediate stopped completion
- `just_from(fn)` - Lazy value from function
- `schedule(scheduler)` - Schedule on execution context
- `transfer_just(scheduler, value)` - Transfer value to scheduler
- `read_env(query)` - Read from environment

### Adaptors (`src/adaptors/`)

- `then(fn)` - Transform value
- `upon_error(fn)` - Transform error
- `upon_stopped(fn)` - Transform stopped
- `let_value(fn)` - Chain dependent sender
- `let_error(fn)` - Handle error with sender
- `let_stopped(fn)` - Handle stopped with sender
- `on(scheduler, sender)` - Run on scheduler
- `starts_on(scheduler, sender)` - Start on scheduler
- `continues_on(scheduler)` - Continue on scheduler
- `when_all(s1, s2)` - Wait for both senders
- `when_any(s1, s2)` - Race two senders
- `split(sender)` - Multi-shot sender
- `ensure_started(sender)` - Eager execution
- `bulk(shape, fn)` - Parallel iteration
- `materialize()` - Completion to variant
- `repeat_n(n)` - Repeat N times
- `repeat_effect_until(pred)` - Repeat with condition
- `finally(cleanup)` - Guaranteed cleanup
- `fork_join2/3/4(fns...)` - Fork and join

### Schedulers (`src/schedulers/`)

- `InlineScheduler` - Synchronous inline execution
- `RunLoop` - Single-threaded event loop
- `SingleThreadContext` - Dedicated thread executor
- `StaticThreadPool` - Multi-threaded work-stealing pool
- `TrampolineScheduler` - Recursion-safe scheduler
- `ManualScheduler` - Manual advance for testing
- `TimedThreadScheduler` - Timer-based scheduling

### Structured Concurrency (`src/scope/`)

- `AsyncScope` - Tracks nested sender lifetimes
- `CountingScope` - Reference-counting scope
- `nest(sender)` - Track sender in scope

### Networking (`src/net/`)

Following the P2762 async I/O proposal:

- `IoContext` - Event loop with mio
- `TcpListener` / `TcpStream` - TCP sockets
- `async_accept()` - Accept connections
- `async_receive()` / `async_send()` - I/O operations

## Benchmark Results

Single-threaded echo server comparison:

| Server | Light (50 conn) | Medium (200) | Heavy (500) | Large (4KB) |
|--------|-----------------|--------------|-------------|-------------|
| **Tokio** | 104,601 req/s | 114,478 | 107,333 | 85,745 |
| **exec-rs** | **114,723 req/s** | **130,694** | **135,157** | **100,321** |

exec-rs outperforms Tokio by **10-26%** in single-threaded mode.

## Known Limitations

- **Nested `when_all`**: The `WhenAllReceiver` types don't implement `Clone`, preventing nested `when_all` calls. Use sequential pairs instead.
- **Sequence senders**: `merge` and `transform_each` are partially implemented.

## References

- [P2300R10 std::execution](https://www.open-std.org/jtc1/sc22/wg21/docs/papers/2024/p2300r10.html)
- [P2762 Sender/Receiver Async I/O](https://wg21.link/p2762)
- [NVIDIA/stdexec](https://github.com/NVIDIA/stdexec) - Reference implementation

## License

Apache-2.0
