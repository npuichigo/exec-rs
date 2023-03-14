pub mod just;

pub fn just<T>(value: T) -> just::Just<T> {
    just::Just::new(value)
}
