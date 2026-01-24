# Echo Server Benchmarks

This directory contains echo server implementations for benchmarking exec-rs against tokio.

## Echo Servers

### 1. tokio_echo
Standard tokio-based async echo server.
```bash
cargo run --example tokio_echo --release
```

### 2. exec_echo
exec-rs with blocking IO and thread pool scheduling.
```bash
cargo run --example exec_echo --release
```

### 3. exec_echo_mio
Single-threaded event-driven echo server using mio.
```bash
cargo run --example exec_echo_mio --release
```

### 4. exec_echo_mt
Multi-threaded exec-rs server with acceptor threads and worker pool.
```bash
cargo run --example exec_echo_mt --release
```

## Benchmarking with rust_echo_bench

### Install the benchmark tool
```bash
git clone https://github.com/haraldh/rust_echo_bench
cd rust_echo_bench
cargo build --release
```

### Run benchmarks

1. Start one of the echo servers (in terminal 1):
```bash
# Option 1: tokio
cargo run --example tokio_echo --release

# Option 2: exec-rs multi-threaded
cargo run --example exec_echo_mt --release
```

2. Run the benchmark (in terminal 2):
```bash
cd rust_echo_bench
./target/release/echo_bench -a 127.0.0.1:12345 -c 100 -l 100 -d 10
```

Parameters:
- `-a`: Server address
- `-c`: Number of concurrent connections
- `-l`: Message length in bytes
- `-d`: Duration in seconds

### Example benchmark commands

Light load:
```bash
./echo_bench -a 127.0.0.1:12345 -c 50 -l 128 -d 10
```

Medium load:
```bash
./echo_bench -a 127.0.0.1:12345 -c 500 -l 128 -d 10
```

Heavy load:
```bash
./echo_bench -a 127.0.0.1:12345 -c 1000 -l 128 -d 10
```

Large messages:
```bash
./echo_bench -a 127.0.0.1:12345 -c 100 -l 4096 -d 10
```

## Architecture Comparison

| Server | IO Model | Threading | Best For |
|--------|----------|-----------|----------|
| tokio_echo | async/await | multi-threaded runtime | High concurrency, many idle connections |
| exec_echo | blocking | thread pool | Simple, moderate concurrency |
| exec_echo_mio | epoll/kqueue | single-threaded | Low latency, CPU-bound |
| exec_echo_mt | blocking | multi-threaded pool | High throughput, CPU-bound |

## Sample Benchmark Results

Test machine: Linux 5.15, **4 worker threads** for fair comparison

### 50 Concurrent Connections (128B messages, 5 seconds)

| Server | Requests/sec | Notes |
|--------|-------------|-------|
| tokio_echo | 1,816k | 4 worker threads |
| exec_echo_mt | 1,917k | 4 workers + 2 acceptors |

### 500 Concurrent Connections (128B messages, 5 seconds)

| Server | Requests/sec | Notes |
|--------|-------------|-------|
| tokio_echo | 3,714k | 4 worker threads |
| exec_echo_mt | 3,703k | 4 workers + 2 acceptors |

**Key Findings:**
- With equal thread counts, tokio and exec-rs perform nearly identically
- Both achieve ~3.7M requests/sec with 500 connections
- exec-rs is slightly faster (~5%) at lower concurrency (50 connections)
- The blocking IO model in exec-rs has minimal overhead for short-lived work

## Notes

- exec-rs is primarily an execution framework (sender/receiver model) not a full async runtime
- For high-performance async IO, consider integrating exec-rs with io_uring
- The blocking IO examples show how exec-rs can coordinate concurrent work
- With equal thread counts, tokio and exec-rs perform nearly identically for this workload
- tokio has the advantage of integrated async IO primitives and is better for:
  - Many idle connections (C10K problem)
  - Complex async workflows
  - Built-in timers, channels, and other async primitives
- exec-rs with blocking IO is simpler and works well when:
  - Work per connection is short and CPU-bound
  - You don't need async primitives
  - You want direct control over threading
