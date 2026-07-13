//! Bytecode format and runtime types for Plotnik.
//!
//! The compiler emits bytecode in memory for the VM. It is not
//! a user-facing file or interchange format. The public views support compiler
//! diagnostics, `plotnik dump`, and other teaching or debugging tools.

#![allow(clippy::comparison_chain)]
// Without the `vm` feature the engine-serving half of this module (the
// pre-decoded instruction stream, trace rendering) has no callers. It stays
// compiled anyway so `Module` is the same object in every configuration —
// load-time validation must not drift between the compiler-only build and the
// executing one.
#![cfg_attr(not(feature = "vm"), allow(dead_code, unused_imports))]

mod aligned_vec;
mod constants;
mod dump;
mod effects;
mod entry_point;
mod format;
mod header;
mod ids;
mod instructions;
mod module;
mod node_kind_constraint;
mod predicate_op;
mod render;
mod sections;
mod spans;
mod type_meta;
pub mod type_system;

pub use dump::dump;
pub use entry_point::EntryPoint;
pub use ids::{StringId, TypeId};
pub use instructions::{CodeAddr, EncodeError};
pub use module::{EntryPointsView, Module, StringsView, TypesView};
pub use spans::{Labeling, SPAN_NO_BINDING, SpanEntry, SpanKind, SpansView};
pub use type_meta::{TypeDef, TypeDefKind, TypeMember, TypeNameEntry};
pub use type_system::{PrimitiveType, TypeKind};

pub(crate) use constants::{
    BYTECODE_WORD_SIZE, HEADER_SIZE, MAGIC, MAX_EFFECTS, MAX_MATCH_PAYLOAD_SLOTS, MAX_NEG_FIELDS,
    MAX_SPANS, REGEX_TABLE_ENTRY_SIZE, SECTION_ALIGN, SPAN_ENTRY_SIZE, STRING_TABLE_ENTRY_SIZE,
    VERSION,
};
pub(crate) use effects::{Effect, EffectKind, EffectSuppression, FrameAction, ValueFrameKind};
pub(crate) use format::{
    LineBuilder, Symbol, cols, nav_symbol, trace, truncate_text, width_for_count,
};
pub(crate) use header::Header;
pub(crate) use instructions::{
    Call, Match, MatchInstr, MatchPredicate, Return, ReturnEntry, ReturnMode, RoutedCall,
    SplitCall, SplitCallReturns, SuccessorAddr, select_match_opcode,
};
pub(crate) use module::{
    DecodedCall, DecodedInstr, DecodedMatch, DecodedPredicate, DecodedRoutedCall, DecodedSplitCall,
    Instruction,
};
pub(crate) use node_kind_constraint::NodeKindConstraint;
// Nav and the regex DFA runtime live in `plotnik-rt` (shared with generated
// code); re-exported here because they are part of the bytecode vocabulary.
pub(crate) use plotnik_rt::{Nav, deserialize_dfa};
pub(crate) use predicate_op::PredicateOp;
pub(crate) use render::ModuleRenderContext;
pub(crate) use sections::{FieldEntry, NodeKindEntry, SymbolNameEntry};

#[cfg(test)]
mod aligned_vec_tests;
#[cfg(test)]
mod effects_tests;
#[cfg(test)]
mod entry_point_tests;
#[cfg(test)]
mod format_tests;
#[cfg(test)]
mod header_tests;
#[cfg(test)]
mod instructions_tests;
#[cfg(test)]
mod node_kind_constraint_tests;
#[cfg(test)]
mod spans_tests;
