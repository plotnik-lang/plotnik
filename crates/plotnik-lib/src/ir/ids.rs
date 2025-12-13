//! ID types for the compiled query IR.
//!
//! These are lightweight wrappers/aliases for indices and identifiers
//! used throughout the IR. They provide type safety without runtime cost.

use std::num::NonZeroU16;

/// Index into the transitions segment.
pub type TransitionId = u32;

/// Node type ID from tree-sitter. Do not change the underlying type.
pub type NodeTypeId = u16;

/// Node field ID from tree-sitter. Uses `NonZeroU16` so `Option<NodeFieldId>`
/// is the same size as `NodeFieldId` (niche optimization with 0 = None).
pub type NodeFieldId = NonZeroU16;

/// Index into the string_refs segment.
pub type StringId = u16;

/// Sentinel value for unnamed types (wrapper types have no explicit name).
pub const STRING_NONE: StringId = 0xFFFF;

/// Field name in effects (alias for type safety).
pub type DataFieldId = StringId;

/// Variant tag in effects (alias for type safety).
pub type VariantTagId = StringId;

/// Index for definition references (Enter/Exit).
pub type RefId = u16;

/// Index into type_defs segment (with reserved primitives 0-2).
pub type TypeId = u16;

// TypeId reserved constants
pub const TYPE_VOID: TypeId = 0;
pub const TYPE_NODE: TypeId = 1;
pub const TYPE_STR: TypeId = 2;
pub const TYPE_INVALID: TypeId = 0xFFFF;
