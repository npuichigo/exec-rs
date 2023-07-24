mod sync_wait;
pub use sync_wait::sync_wait;

mod into_awaitable;
pub mod start_detached;
pub mod submit;

pub use into_awaitable::SenderAwaitable;
pub use start_detached::start_detached;
pub use submit::submit;
