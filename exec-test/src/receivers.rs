use exec_core::receiver::{SetError, SetValue};
use std::error::Error;
use std::fmt::Debug;

pub struct ExpectValueReceiver<T> {
    expected: T,
}

impl<T> ExpectValueReceiver<T> {
    pub fn new(expected: T) -> Self {
        Self { expected }
    }
}

impl<T> SetValue for ExpectValueReceiver<T>
where
    T: PartialEq + Debug,
{
    type Value = T;

    fn set_value(self, value: Self::Value) {
        println!("Expected: {:?}, Actual: {:?}", self.expected, value);
        assert_eq!(self.expected, value);
    }
}

pub struct ExpectErrorReceiver<E> {
    expected: E,
}

impl<E> ExpectErrorReceiver<E> {
    pub fn new(expected: E) -> Self {
        Self { expected }
    }
}

impl<E: Error + PartialEq> SetError for ExpectErrorReceiver<E> {
    type Error = E;

    fn set_error(self, error: Self::Error) {
        println!("Expected: {:?}, Actual: {:?}", self.expected, error);
        assert_eq!(self.expected, error);
    }
}
