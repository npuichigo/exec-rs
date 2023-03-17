#[macro_use]
mod macros;

mod utils;

mod run_loop;

pub trait Executable {
    fn execute(&mut self);
}
