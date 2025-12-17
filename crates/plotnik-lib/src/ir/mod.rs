//! Intermediate Representation (IR) for compiled queries.
//!
//! This module contains the in-memory representation of compiled queries
//! as defined in ADR-0004 through ADR-0008. The IR is designed for:
//! - Cache-efficient execution (64-byte aligned transitions)
//! - Zero-copy access patterns
//! - WASM compatibility
//!
//! Note: This module contains only type definitions. Query execution
//! lives elsewhere.

mod compiled;
mod effect;
mod emit;
mod entrypoint;
mod ids;
mod matcher;
mod nav;
mod ref_transition;
mod serialize;
mod slice;
mod string_ref;
mod strings;
mod transition;
mod type_metadata;

#[cfg(test)]
mod effect_tests;
#[cfg(test)]
mod matcher_tests;
#[cfg(test)]
mod ref_transition_tests;
#[cfg(test)]
mod slice_tests;
#[cfg(test)]
mod string_ref_tests;

// Re-export ID types
pub use ids::{DataFieldId, RefId, STRING_NONE, StringId, TransitionId, TypeId, VariantTagId};

// Re-export TypeId constants
pub use ids::{TYPE_INVALID, TYPE_NODE, TYPE_STR, TYPE_VOID};

// Re-export Slice
pub use slice::Slice;

// Re-export navigation
pub use nav::{Nav, NavKind};

// Re-export matcher
pub use matcher::{Matcher, MatcherKind};

// Re-export effects
pub use effect::EffectOp;

// Re-export ref transition
pub use ref_transition::RefTransition;

// Re-export transition
pub use transition::{MAX_INLINE_SUCCESSORS, Transition};

// Re-export type metadata
pub use type_metadata::{TYPE_COMPOSITE_START, TypeDef, TypeKind, TypeMember};

// Re-export string ref
pub use string_ref::StringRef;

// Re-export entrypoint
pub use entrypoint::Entrypoint;

// Re-export compiled query types
pub use compiled::{
    BUFFER_ALIGN, CompiledQuery, CompiledQueryBuffer, CompiledQueryOffsets, FORMAT_VERSION, MAGIC,
    MatcherView, TransitionView, align_up,
};

// Re-export string interner
pub use strings::StringInterner;

// Re-export emitter
pub use emit::{EmitError, EmitResult, MapResolver, NodeKindResolver, NullResolver, QueryEmitter};

// Re-export serialization
pub use serialize::{
    HEADER_SIZE, SerializeError, SerializeResult, deserialize, from_bytes, serialize, to_bytes,
};
