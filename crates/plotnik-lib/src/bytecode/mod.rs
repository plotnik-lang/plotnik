//! Bytecode format and runtime types for Plotnik.
//!
//! Public bytecode facade for compiled Plotnik queries. Implements the binary
//! format specified in `docs/binary-format/`.

#![allow(clippy::comparison_chain)]

mod aligned_vec;
mod constants;
mod dfa;
mod dump;
mod effects;
mod entrypoint;
mod format;
mod header;
mod ids;
mod instructions;
mod module;
mod nav;
mod node_kind_constraint;
mod predicate_op;
mod render;
mod sections;
mod spans;
mod type_meta;
pub mod type_system;

pub use dump::dump;
pub use entrypoint::Entrypoint;
pub use ids::{StringId, TypeId};
pub use instructions::{EncodeError, StepAddr};
pub use module::{EntrypointsView, Module, ModuleError, StringsView, TypesView};
pub use spans::{SPAN_NO_BINDING, SpanEntry, SpanKind, SpansView};
pub use type_meta::{TypeDef, TypeDefKind, TypeMember, TypeNameEntry};
pub use type_system::{Arity, PrimitiveType, TypeKind};

pub(crate) use constants::{
    HEADER_SIZE, MAGIC, MAX_EFFECTS, MAX_MATCH_PAYLOAD_SLOTS, MAX_NEG_FIELDS, MAX_SPANS,
    REGEX_TABLE_ENTRY_SIZE, SECTION_ALIGN, SPAN_ENTRY_SIZE, STEP_SIZE, STRING_TABLE_ENTRY_SIZE,
    VERSION,
};
pub(crate) use dfa::deserialize_dfa;
pub(crate) use effects::{Effect, EffectKind};
pub(crate) use format::{
    LineBuilder, Symbol, cols, nav_symbol, trace, truncate_text, width_for_count,
};
pub(crate) use header::Header;
pub(crate) use instructions::{
    Call, Match, MatchInstr, MatchPredicate, Return, StepId, select_match_opcode,
};
pub(crate) use module::{DecodedCall, DecodedInstr, DecodedMatch, DecodedPredicate, Instruction};
pub(crate) use nav::Nav;
pub(crate) use node_kind_constraint::NodeKindConstraint;
pub(crate) use predicate_op::PredicateOp;
pub(crate) use render::ModuleRenderContext;
pub(crate) use sections::{FieldEntry, NodeKindEntry, SymbolNameEntry};

#[cfg(test)]
mod aligned_vec_tests;
#[cfg(test)]
mod dfa_tests;
#[cfg(test)]
mod effects_tests;
#[cfg(test)]
mod entrypoint_tests;
#[cfg(test)]
mod format_tests;
#[cfg(test)]
mod header_tests;
#[cfg(test)]
mod instructions_tests;
#[cfg(test)]
mod nav_tests;
#[cfg(test)]
mod node_kind_constraint_tests;
#[cfg(test)]
mod spans_tests;
#[cfg(test)]
mod type_meta_tests;
