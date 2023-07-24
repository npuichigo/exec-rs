mod adaptors;
pub use adaptors::then;

mod consumers;
pub use consumers::start_detached;
pub use consumers::submit;
pub use consumers::sync_wait;

mod factories;
pub use factories::just;
