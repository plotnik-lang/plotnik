//! Effect operations for the query IR.
//!
//! Effects are recorded during transition execution and replayed
//! during materialization to construct the output value.

use super::ids::{DataFieldId, VariantTagId};

/// Effect operation in the IR effect stream.
///
/// Effects are executed sequentially after a successful match.
/// They manipulate a value stack to construct structured output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, u16)]
pub enum EffectOp {
    /// Store matched node as current value.
    /// Only valid on transitions with Node/Anonymous/Wildcard matcher.
    CaptureNode,

    /// Clear current value (set to None).
    /// Used on skip paths for optional captures.
    ClearCurrent,

    /// Push empty array onto stack.
    StartArray,

    /// Move current value into top array.
    PushElement,

    /// Pop array from stack into current.
    EndArray,

    /// Push empty object onto stack.
    StartObject,

    /// Pop object from stack into current.
    EndObject,

    /// Move current value into top object at field.
    Field(DataFieldId),

    /// Push variant container with tag onto stack.
    StartVariant(VariantTagId),

    /// Pop variant, wrap current, set as current.
    EndVariant,

    /// Replace current Node with its source text.
    ToString,
}
