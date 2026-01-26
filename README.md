# exec-rs Implementation Status Report

A Rust implementation of C++ std::execution (P2300) based on the NVIDIA/stdexec reference implementation.

## Summary

- **Source Files**: 70+ Rust files
- **Unit Tests**: 195 passing
- **Doc Tests**: 44 passing (26 ignored)
- **Examples**: 2 echo server implementations + benchmark

---

## Phase Completion Status

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
| 14 | Utilities | ⚠️ Partial |
| 15 | Future Bridge | ✅ Complete |
| 16 | **Networking (P2762)** | ✅ Complete |

---

## Detailed Implementation Status

### Phase 1: Core Traits (`src/traits/`)

| Component | File | Status |
|-----------|------|--------|
| `Sender` trait | `sender.rs` | ✅ |
| `SenderTo<R>` trait | `sender.rs` | ✅ |
| `SenderExt` extension | `sender.rs` | ✅ |
| `Receiver` trait | `receiver.rs` | ✅ |
| `ReceiverOf<V>` trait | `receiver.rs` | ✅ |
| `ReceiverOfError<E>` trait | `receiver.rs` | ✅ |
| `ReceiverOfStopped` trait | `receiver.rs` | ✅ |
| `OperationState` trait | `operation.rs` | ✅ |
| `CompletionSignatures` trait | `completion.rs` | ✅ |
| `Scheduler` trait | `scheduler.rs` | ✅ |
| `Environment` trait | `env.rs` | ✅ |
| `GetScheduler` query | `env.rs` | ✅ |
| `GetStopToken` query | `env.rs` | ✅ |

### Phase 1: Stop Tokens (`src/stop/`)

| Component | File | Status |
|-----------|------|--------|
| `StopToken` trait | `token.rs` | ✅ |
| `StopSource` trait | `source.rs` | ✅ |
| `InplaceStopSource` | `inplace.rs` | ✅ |
| `InplaceStopToken` | `inplace.rs` | ✅ |
| `StopCallback` | `callback.rs` | ✅ |
| `NeverStopToken` | `never.rs` | ✅ |

### Phase 2: Sender Factories (`src/factories/`)

| Component | File | Status |
|-----------|------|--------|
| `just(value)` | `just.rs` | ✅ |
| `just_error(error)` | `just.rs` | ✅ |
| `just_stopped()` | `just.rs` | ✅ |
| `just_from(fn)` | `just.rs` | ✅ |
| `just_error_from(fn)` | `just.rs` | ✅ |
| `just_stopped_from(fn)` | `just.rs` | ✅ |
| `schedule(scheduler)` | `schedule.rs` | ✅ |
| `transfer_just(scheduler, value)` | `schedule.rs` | ✅ |
| `read_env(query)` | `read_env.rs` | ✅ |

### Phase 3: Consumers (`src/consumers/`)

| Component | File | Status |
|-----------|------|--------|
| `sync_wait(sender)` | `sync_wait.rs` | ✅ |
| `start_detached(sender)` | `start_detached.rs` | ✅ |

### Phase 4-5: Adaptors (`src/adaptors/`)

| Component | File | Status |
|-----------|------|--------|
| `then(sender, fn)` | `then.rs` | ✅ |
| `upon_error(sender, fn)` | `upon_error.rs` | ✅ |
| `upon_stopped(sender, fn)` | `upon_stopped.rs` | ✅ |
| `let_value(sender, fn)` | `let_value.rs` | ✅ |
| `let_error(sender, fn)` | `let_error.rs` | ✅ |
| `let_stopped(sender, fn)` | `let_stopped.rs` | ✅ |

### Phase 6: Execution Context Switching

| Component | File | Status |
|-----------|------|--------|
| `on(scheduler, sender)` | `on.rs` | ✅ |
| `starts_on(scheduler, sender)` | `starts_on.rs` | ✅ |
| `continues_on(scheduler)` | `continues_on.rs` | ✅ |
| `reschedule()` | `reschedule.rs` | ✅ |

### Phase 7: Concurrent Composition

| Component | File | Status |
|-----------|------|--------|
| `when_all(s1, s2, ...)` | `when_all.rs` | ✅ |
| `when_any(s1, s2, ...)` | `when_any.rs` | ✅ |

### Phase 8: Additional Adaptors

| Component | File | Status |
|-----------|------|--------|
| `split(sender)` | `split.rs` | ✅ |
| `ensure_started(sender)` | `ensure_started.rs` | ✅ |
| `bulk(sender, shape, fn)` | `bulk.rs` | ✅ |
| `materialize()` | `materialize.rs` | ✅ |
| `into_tuple()` | `into_tuple.rs` | ✅ |
| `stopped_as_optional()` | `stopped_as_optional.rs` | ✅ |
| `stopped_as_error(err)` | `stopped_as_error.rs` | ✅ |
| `repeat_effect(sender)` | `repeat.rs` | ✅ |
| `repeat_n(sender, n)` | `repeat.rs` | ✅ |
| `repeat_effect_until(sender, pred)` | `repeat.rs` | ✅ |
| `finally(sender, cleanup)` | `finally.rs` | ✅ |
| `unless_stopped()` | `unless_stopped.rs` | ✅ |
| `into_variant()` | `into_variant.rs` | ✅ |
| `write_env(sender, env)` | `write_env.rs` | ✅ |
| `fork_join2/3/4(sender, fns...)` | `fork_join.rs` | ✅ |

### Phase 9: Structured Concurrency (`src/scope/`)

| Component | File | Status |
|-----------|------|--------|
| `AsyncScope` | `async_scope.rs` | ✅ |
| `CountingScope` | `counting_scope.rs` | ✅ |
| `nest(sender)` | `nest.rs` | ✅ |

### Phase 11: Type-Erased Senders (`src/any/`)

| Component | File | Status |
|-----------|------|--------|
| `AnySender<V, E>` | `any_sender.rs` | ✅ |
| `AnyScheduler` | `any_scheduler.rs` | ✅ |
| `VariantSender2<S1, S2>` | `variant_sender.rs` | ✅ |
| `VariantSender3<S1, S2, S3>` | `variant_sender.rs` | ✅ |
| `VariantSender4<S1, S2, S3, S4>` | `variant_sender.rs` | ✅ |
| `EitherSender` (alias) | `variant_sender.rs` | ✅ |

### Phase 12: Schedulers (`src/schedulers/`)

| Component | File | Status |
|-----------|------|--------|
| `InlineScheduler` | `inline.rs` | ✅ |
| `RunLoop` | `run_loop.rs` | ✅ |
| `SingleThreadContext` | `single_thread.rs` | ✅ |
| `StaticThreadPool` | `thread_pool.rs` | ✅ |
| `TrampolineScheduler` | `trampoline.rs` | ✅ |
| `ManualScheduler` | `manual.rs` | ✅ |
| `TimedScheduler` trait | `timed.rs` | ✅ |
| `TimedThreadContext` | `timed_thread.rs` | ✅ |
| `TimedThreadScheduler` | `timed_thread.rs` | ✅ |

### Phase 14: Utilities (`src/util/`)

| Component | File | Status |
|-----------|------|--------|
| `ScopeGuard` | `scope_guard.rs` | ✅ |
| `IntrusiveHeap` | `intrusive_heap.rs` | ✅ |
| `AtomicIntrusiveQueue` | `intrusive_queue.rs` | ✅ |

### Phase 13: Coroutine Integration (`src/coro/`)

| Component | File | Status |
|-----------|------|--------|
| `Task<T, E>` | `task.rs` | ✅ |
| `TaskError<E>` | `task.rs` | ✅ |
| `TaskReceiver<T, E>` | `task.rs` | ✅ |
| `SpawnExt` trait | `task.rs` | ✅ |
| `Awaitable` trait | `awaitable.rs` | ✅ |
| `ReadyAwaitable` trait | `awaitable.rs` | ✅ |
| `at_coroutine_exit(fn)` | `at_exit.rs` | ✅ |
| `scope_exit(fn)` | `at_exit.rs` | ✅ |
| `DeferredCleanup` | `at_exit.rs` | ✅ |

### Phase 15: Future Bridge (`src/future/`)

| Component | File | Status |
|-----------|------|--------|
| `SenderToFutureExt` trait | `sender_to_future.rs` | ✅ |
| `SenderFuture<S>` | `sender_to_future.rs` | ✅ |
| `SenderFutureError<E>` | `sender_to_future.rs` | ✅ |
| `FutureReceiver<V, E>` | `sender_to_future.rs` | ✅ |
| `from_future(F)` | `future_to_sender.rs` | ✅ |
| `FutureSender<F>` | `future_to_sender.rs` | ✅ |
| `FutureToSenderExt` trait | `future_to_sender.rs` | ✅ |

### Phase 16: Networking (`src/net/`)

Following the P2762 async I/O proposal pattern with sender-based operations.

| Component | File | Status |
|-----------|------|--------|
| `IoContext` | `io_context.rs` | ✅ |
| `IoContextScheduler` | `io_context.rs` | ✅ |
| `TcpListener` | `socket.rs` | ✅ |
| `TcpStream` | `socket.rs` | ✅ |
| `async_accept(listener, ctx)` | `operations.rs` | ✅ |
| `async_accept_arc(listener, ctx)` | `operations.rs` | ✅ |
| `async_receive(stream, ctx, buf)` | `operations.rs` | ✅ |
| `async_receive_into(client, ctx)` | `operations.rs` | ✅ |
| `async_send(stream, ctx, buf)` | `operations.rs` | ✅ |
| `async_send_from(client, ctx)` | `operations.rs` | ✅ |
| `ClientState` | `operations.rs` | ✅ |
| `Buffer` / `MutBuffer` traits | `buffer.rs` | ✅ |

---

## Not Yet Implemented

### Phase 10: Sequence Senders (Partial)
- ✅ `SequenceSender` trait - `traits.rs`
- ✅ `iterate(range)` - `iterate.rs`
- ✅ `empty_sequence()` - `empty.rs`
- ⚠️ `merge(seq1, seq2)` - Not implemented
- ⚠️ `transform_each(seq, adaptor)` - Partial (wrapper only)
- ⚠️ `ignore_all_values(seq)` - Partial (wrapper only)

---

## Examples (`examples/`)

| Example | Description |
|---------|-------------|
| `tokio_echo.rs` | Tokio-based echo server (baseline) |
| `exec_echo.rs` | exec-rs async sender/receiver + AsyncScope |
| `benchmark.sh` | Automated benchmark script |

### Benchmark Results (Single-threaded, 1 worker thread each)

| Server | Light (50 conn) | Medium (200) | Heavy (500) | Large (4KB) |
|--------|-----------------|--------------|-------------|-------------|
| **Tokio** | 104,601 req/s | 114,478 | 107,333 | 85,745 |
| **exec-rs Async** | **114,723 req/s** | **130,694** | **135,157** | **100,321** |

**exec-rs async outperforms Tokio by 10-26% in single-threaded mode**, demonstrating the efficiency of the sender/receiver model with direct mio integration.

### Echo Server Architecture (exec_echo.rs)

```
repeat_effect(
    async_accept(server)
    | then(|(stream, addr)| { scope.spawn(echo_client); })
)

// Client echo loop (runs until EOF)
repeat_effect_until(
    async_receive_into(client)
       | then(|n| { client.done = (n == 0); })
       | let_value(|| async_send_from(client)),
    || client.done
)
```

Key features:
- **Structured concurrency**: AsyncScope tracks all client handlers
- **Sender composition**: `repeat_effect`, `then`, `let_value` chain
- **Clone-based iteration**: Senders cloned each iteration (matching stdexec)
- **mio integration**: Direct epoll/kqueue event loop

---

## Architecture Highlights

### Core Design Patterns
1. **Sender/Receiver Model**: Lazy async work representation
2. **Operation State**: Pinned state machine for execution
3. **Completion Signatures**: Type-level encoding of possible outcomes
4. **Environment Queries**: Extensible context propagation

### Key Features
- **Zero-cost abstractions**: No heap allocation for basic operations
- **Type-safe composition**: Compile-time checked sender pipelines
- **Structured concurrency**: Scopes ensure proper lifetime management
- **Cancellation support**: Stop tokens propagate through pipelines
- **Async I/O**: mio-based networking with sender operations

### Thread Safety
- All schedulers are `Send + Sync`
- Thread pool uses work-stealing for load balancing
- Proper condvar synchronization (no busy-waiting)

---

## Code Quality

- **195 unit tests** covering all major functionality
- **44 doc tests** with usage examples (26 ignored)
- **Warnings**: Minimal
- **Unsafe code**: Minimized, only where necessary for pinning and interior mutability

---

## References

- [P2300R10 Specification](https://www.open-std.org/jtc1/sc22/wg21/docs/papers/2024/p2300r10.html)
- [P2762 Sender/Receiver Async I/O](https://wg21.link/p2762)
- [NVIDIA/stdexec](https://github.com/NVIDIA/stdexec) - Reference implementation
- [kuhllib echo_server](https://github.com/kirkshoop/kuhllib/blob/main/src/examples/echo_server.cpp) - Pattern reference
