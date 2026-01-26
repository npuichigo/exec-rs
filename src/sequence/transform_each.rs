//! transform_each - applies an adaptor to each item in a sequence.

use super::traits::SequenceSender;

/// Transforms each item in a sequence by applying a function.
///
/// The function receives the item sender and should return a new sender.
pub fn transform_each<S, F>(sequence: S, transform: F) -> TransformEach<S, F>
where
    S: SequenceSender,
{
    TransformEach {
        sequence,
        transform,
    }
}

/// A sequence sender that transforms each item.
pub struct TransformEach<S, F> {
    /// The inner sequence.
    pub sequence: S,
    /// The transform function.
    pub transform: F,
}

impl<S: SequenceSender, F> SequenceSender for TransformEach<S, F> {
    type ItemTypes = S::ItemTypes;
    type Completions = S::Completions;
}

/// Extension trait for transform_each.
pub trait TransformEachExt: SequenceSender + Sized {
    /// Transform each item in this sequence.
    fn transform_each<F>(self, transform: F) -> TransformEach<Self, F> {
        transform_each(self, transform)
    }
}

impl<S: SequenceSender> TransformEachExt for S {}

// Note: Full implementation requires more complex type machinery
// to properly track transformed sender types through the sequence.
