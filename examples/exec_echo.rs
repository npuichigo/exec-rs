//! Async echo server using exec-rs sender/receiver model.
//!
//! This example demonstrates a sender-based echo server following the stdexec/kuhllib pattern
//! with structured concurrency via AsyncScope:
//!
//! Usage: cargo run --example exec_echo_async --release
//!
//! Test with: echo "hello" | nc localhost 12345

use exec_rs::adaptors::{let_value, repeat_effect, repeat_effect_until, then};
use exec_rs::net::{
    async_accept_arc, async_receive_into, async_send_from, ClientState, IoContext, TcpListener,
};
use exec_rs::scope::AsyncScope;
use std::io;
use std::sync::Arc;

fn main() -> io::Result<()> {
    let addr = "127.0.0.1:12345";

    // Create IoContext - the async I/O runtime
    let ctx = Arc::new(IoContext::new()?);
    let listener = Arc::new(TcpListener::bind(&ctx, addr)?);

    // Create AsyncScope for structured concurrency - tracks all client handlers
    let scope = AsyncScope::new();

    println!("exec-rs async echo server listening on {}", addr);

    // Build the accept sender - cloned each iteration by repeat_effect
    let ctx_for_spawn = ctx.clone();
    let scope_for_spawn = scope.clone();

    let accept_sender = then(
        async_accept_arc(listener.clone(), ctx.clone()),
        move |(stream, _addr)| {

            // Build the echo client sender inline
            let client = ClientState::new(stream, 4096);
            let client_for_predicate = client.clone();
            let client_for_then = client.clone();
            let client_for_send = client.clone();
            let ctx_for_send = ctx_for_spawn.clone();

            // Echo iteration sender - cloned each iteration by repeat_effect_until
            let echo_iteration = let_value(
                then(async_receive_into(client, ctx_for_spawn.clone()), move |n| {
                    client_for_then.done.set(n == 0);
                }),
                move |_| {
                    then(
                        async_send_from(client_for_send.clone(), ctx_for_send.clone()),
                        |_sent| {},
                    )
                },
            );

            let predicate = move || client_for_predicate.done.get();
            let echo_sender = repeat_effect_until(echo_iteration, predicate);

            // Spawn the echo client handler using AsyncScope for structured concurrency
            scope_for_spawn.spawn(echo_sender);
        },
    );

    // repeat_effect clones the accept_sender each iteration
    let accept_loop = repeat_effect(accept_sender);

    // Spawn the accept loop using AsyncScope
    scope.spawn(accept_loop);


    // Run the IoContext event loop
    loop {
        match ctx.run_one() {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {
                scope.request_stop();
                break;
            }
            Err(e) => {
                eprintln!("IoContext error: {}", e);
                break;
            }
        }
    }

    Ok(())
}
