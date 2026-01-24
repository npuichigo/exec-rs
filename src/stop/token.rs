//! Stop token trait definition.

/// The stop token trait.
///
/// A stop token is a non-owning handle that can be used to check if a stop
/// has been requested and to register callbacks that will be invoked when
/// a stop is requested.
///
/// # Thread Safety
///
/// Stop tokens must be `Clone` and `Send + Sync` to allow sharing across
/// threads. All operations on stop tokens are thread-safe.
pub trait StopToken: Clone + Send + Sync + 'static {
    /// The callback type for this stop token.
    type Callback<F: FnOnce() + Send>: StopCallbackHandle;

    /// Check if a stop has been requested.
    ///
    /// Returns `true` if `request_stop()` has been called on the associated
    /// stop source.
    fn stop_requested(&self) -> bool;

    /// Check if a stop is possible.
    ///
    /// Returns `true` if this token is associated with a stop source that
    /// could potentially request a stop. Returns `false` for tokens like
    /// `NeverStopToken` where stops are never possible.
    fn stop_possible(&self) -> bool;

    /// Register a callback to be invoked when stop is requested.
    ///
    /// If stop has already been requested, the callback is invoked immediately.
    /// The returned handle keeps the callback registered; dropping it
    /// unregisters the callback.
    fn register_callback<F: FnOnce() + Send + 'static>(&self, callback: F) -> Self::Callback<F>;
}

/// Handle for a registered stop callback.
///
/// Dropping this handle unregisters the callback. The callback is guaranteed
/// to either:
/// - Have already executed before the drop returns
/// - Be removed without executing
pub trait StopCallbackHandle: Send {
    /// Check if the callback has been invoked.
    fn was_invoked(&self) -> bool;
}

/// A simple callback handle that does nothing on drop.
#[derive(Debug)]
pub struct NoopCallbackHandle {
    invoked: bool,
}

impl NoopCallbackHandle {
    /// Create a new handle.
    pub fn new(invoked: bool) -> Self {
        Self { invoked }
    }
}

impl StopCallbackHandle for NoopCallbackHandle {
    fn was_invoked(&self) -> bool {
        self.invoked
    }
}
