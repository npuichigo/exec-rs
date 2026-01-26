//! ignore_all_values - consumes a sequence sender and ignores all item values.

use super::traits::SequenceSender;
use crate::traits::{Sender, ValueCompletion};

/// Consumes a sequence sender and ignores all emitted values.
///
/// This adaptor subscribes to a sequence sender with a receiver that discards all
/// item values, only propagating the final completion or any errors.
pub fn ignore_all_values<S>(sequence: S) -> IgnoreAllValues<S>
where
    S: SequenceSender,
{
    IgnoreAllValues { sequence }
}

/// A sender that ignores all values from a sequence.
pub struct IgnoreAllValues<S> {
    sequence: S,
}

impl<S: SequenceSender> Sender for IgnoreAllValues<S> {
    type Completions = ValueCompletion<()>;
}

/// Extension trait for ignore_all_values.
pub trait IgnoreAllValuesExt: SequenceSender + Sized {
    /// Ignore all values from this sequence.
    fn ignore_all_values(self) -> IgnoreAllValues<Self> {
        ignore_all_values(self)
    }
}

impl<S: SequenceSender> IgnoreAllValuesExt for S {}

// Note: Full implementation requires resolving complex recursive type bounds.
// For now, users can use the sequence directly with a custom receiver.
// The ignore_all_values function creates the wrapper type but full SenderTo
// implementation requires more type machinery.
