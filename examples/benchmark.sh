#!/bin/bash

# Echo Server Benchmark Script
#
# Prerequisites:
# 1. Build rust_echo_bench:
#    git clone https://github.com/haraldh/rust_echo_bench /tmp/rust_echo_bench
#    cd /tmp/rust_echo_bench && cargo build --release
#
# 2. Build exec-rs examples:
#    cargo build --examples --release

set -e

BENCH_BIN="${BENCH_BIN:-/tmp/rust_echo_bench/target/release/echo_bench}"
SERVER_ADDR="127.0.0.1:12345"
DURATION=10
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

# Check if benchmark binary exists
if [ ! -f "$BENCH_BIN" ]; then
    echo "Benchmark binary not found at $BENCH_BIN"
    echo "Please build it first:"
    echo "  git clone https://github.com/haraldh/rust_echo_bench /tmp/rust_echo_bench"
    echo "  cd /tmp/rust_echo_bench && cargo build --release"
    exit 1
fi

# Build examples
echo "Building examples..."
cd "$PROJECT_DIR"
cargo build --examples --release

run_benchmark() {
    local name=$1
    local connections=$2
    local msg_length=$3

    echo ""
    echo "=== $name: $connections connections, ${msg_length}B messages, ${DURATION}s ==="
    "$BENCH_BIN" -a "$SERVER_ADDR" -c "$connections" -l "$msg_length" -t "$DURATION"
}

run_server_benchmark() {
    local server_name=$1
    local example_name=$2

    echo ""
    echo "========================================"
    echo "Benchmarking: $server_name"
    echo "========================================"

    # Start server in background
    "$PROJECT_DIR/target/release/examples/$example_name" &
    SERVER_PID=$!

    # Wait for server to start
    sleep 1

    # Check if server is running
    if ! kill -0 $SERVER_PID 2>/dev/null; then
        echo "Server failed to start!"
        return 1
    fi

    # Run benchmarks
    run_benchmark "Light load" 50 128
    run_benchmark "Medium load" 200 128
    run_benchmark "Heavy load" 500 128
    run_benchmark "Large messages" 100 4096

    # Stop server
    kill $SERVER_PID 2>/dev/null || true
    wait $SERVER_PID 2>/dev/null || true

    sleep 1
}

echo "============================================"
echo "Echo Server Benchmark Suite"
echo "============================================"
echo "Server address: $SERVER_ADDR"
echo "Duration per test: ${DURATION}s"
echo ""

# Run benchmarks for each server
run_server_benchmark "Tokio Echo Server" "tokio_echo"
run_server_benchmark "exec-rs Multi-threaded" "exec_echo_mt"
run_server_benchmark "exec-rs Blocking IO" "exec_echo"
run_server_benchmark "exec-rs Mio (single-threaded)" "exec_echo_mio"

echo ""
echo "========================================"
echo "Benchmark complete!"
echo "========================================"
