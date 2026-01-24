//! Stop source trait definition.

use crate::stop::StopToken;

/// The stop source trait.
///
/// A stop source owns the stop state and can issue stop requests. It provides
/// tokens that can be used to check for stop requests and register callbacks.
///
/// # Thread Safety
///
/// Stop sources are thread-safe. Multiple threads can call `request_stop()`
/// and only one will actually perform the stop (returning `true`).
pub trait StopSource: Send + Sync {
    /// The token type for this stop source.
    type Token: StopToken;

    /// Get a stop token associated with this source.
    ///
    /// The token can be used to check for stop requests and register callbacks.
    fn get_token(&self) -> Self::Token;

    /// Request a stop.
    ///
    /// Returns `true` if this call actually requested the stop (i.e., this was
    /// the first call to `request_stop()`). Returns `false` if a stop was
    /// already requested.
    ///
    /// All registered callbacks are invoked synchronously before this method
    /// returns.
    fn request_stop(&self) -> bool;

    /// Check if a stop has been requested.
    fn stop_requested(&self) -> bool;
}
