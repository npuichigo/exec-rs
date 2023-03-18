pub trait Receiver {}

pub trait SetValue {
    type Value;

    fn set_value(self, value: Self::Value);
}

pub trait SetError {
    type Error;

    fn set_error(self, error: Self::Error);
}

pub trait SetStopped {
    fn set_stopped(self);
}
