# Echo Server Benchmarks

This directory contains echo server implementations for benchmarking exec-rs against tokio.

## Echo Servers

### 1. tokio_echo
Standard tokio-based async echo server.
```bash
cargo run --example tokio_echo --release
```

### 2. exec_echo
exec-rs sender/receiver model with structured concurrency
```bash
cargo run --example exec_echo --release
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

# Option 2: exec-rs
cargo run --example exec_echo --release
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

Test machine: Linux 5.15

### Single-Threaded Performance (1 worker thread)

#### 50 Concurrent Connections (128B messages, 5 seconds)

| Server | Requests/sec | Notes |
|--------|-------------|-------|
| tokio_echo | 1,772k | 1 worker thread |
| exec_echo_mt | 1,850k | 1 worker + 1 acceptor |
| exec_echo_mio | 1,892k | single-threaded event loop |

#### 500 Concurrent Connections (128B messages, 5 seconds)

| Server | Requests/sec | Notes |
|--------|-------------|-------|
| tokio_echo | 3,637k | 1 worker thread |
| exec_echo_mt | 3,744k | 1 worker + 1 acceptor |
| exec_echo_mio | 3,781k | single-threaded event loop |

### Multi-Threaded Performance (4 worker threads)

#### 50 Concurrent Connections (128B messages, 5 seconds)

| Server | Requests/sec | Notes |
|--------|-------------|-------|
| tokio_echo | 1,816k | 4 worker threads |
| exec_echo_mt | 1,917k | 4 workers + 2 acceptors |

#### 500 Concurrent Connections (128B messages, 5 seconds)

| Server | Requests/sec | Notes |
|--------|-------------|-------|
| tokio_echo | 3,714k | 4 worker threads |
| exec_echo_mt | 3,703k | 4 workers + 2 acceptors |

**Key Findings:**
- **Single-threaded**: exec_echo_mio (event loop) is fastest (+7% vs Tokio)
- **Multi-threaded**: Tokio and exec-rs perform nearly identically
- All implementations scale ~2x from 50→500 connections
- Event-driven mio has lowest overhead for simple echo workload
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
