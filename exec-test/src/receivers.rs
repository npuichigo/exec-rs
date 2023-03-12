use exec_core::receiver::SetValue;
use std::fmt::Debug;

pub struct ExpectReceiver<T> {
    expected: T,
}

impl<T> ExpectReceiver<T> {
    pub fn new(expected: T) -> Self {
        Self { expected }
    }
}

impl<T> SetValue for ExpectReceiver<T>
where
    T: PartialEq + Debug,
{
    type Value = T;

    fn set_value(self, value: Self::Value) {
        assert_eq!(self.expected, value);
    }
}
