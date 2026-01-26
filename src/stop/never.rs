//! Never stop token - a token that never signals stop.

use crate::stop::{StopCallbackHandle, StopToken};

/// A stop token that never signals stop.
///
/// This is used as a default when no cancellation is needed. It allows the
/// compiler to optimize away stop-related code paths.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct NeverStopToken;

impl StopToken for NeverStopToken {
    type Callback<F: FnOnce() + Send> = NeverCallback;

    fn stop_requested(&self) -> bool {
        false
    }

    fn stop_possible(&self) -> bool {
        false
    }

    fn register_callback<F: FnOnce() + Send + 'static>(&self, _callback: F) -> Self::Callback<F> {
        NeverCallback
    }
}

/// Callback handle for NeverStopToken.
///
/// Since stops never happen, this callback is never invoked.
#[derive(Debug)]
pub struct NeverCallback;

impl StopCallbackHandle for NeverCallback {
    fn was_invoked(&self) -> bool {
        false
    }
}
