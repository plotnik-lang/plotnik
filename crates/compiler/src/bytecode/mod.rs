//! Compiler-owned, in-memory bytecode, not a user-facing file or interchange format.

#![allow(clippy::comparison_chain)]
#![cfg_attr(not(feature = "vm"), allow(dead_code, unused_imports))]

mod aligned_vec;
mod constants;
mod dump;
mod effects;
mod entrypoint;
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
pub use entrypoint::{Entrypoint, EntrypointIdentity};
pub use ids::{StringId, TypeId};
pub use instructions::{CodeAddr, EncodeError};
pub use module::{EntrypointsView, Module, StringsView, TypesView};
pub use spans::{Labeling, SPAN_NO_BINDING, SpanEntry, SpanKind, SpansView};
pub use type_meta::{TypeDef, TypeDefKind, TypeMember, TypeNameEntry};
pub use type_system::{PrimitiveType, TypeKind};

pub(crate) use constants::{
    BYTECODE_WORD_SIZE, HEADER_SIZE, MAGIC, MAX_EFFECTS, MAX_MATCH_PAYLOAD_SLOTS, MAX_NEG_FIELDS,
    MAX_SPANS, REGEX_TABLE_ENTRY_SIZE, SECTION_ALIGN, SPAN_ENTRY_SIZE, STRING_TABLE_ENTRY_SIZE,
    VERSION,
};
pub(crate) use effects::{Effect, EffectKind, EffectSuppression, FrameAction, ValueFrameKind};
pub(crate) use entrypoint::EntryBoundary;
pub(crate) use format::{
    LineBuilder, Symbol, cols, nav_symbol, trace, truncate_text, width_for_count,
};
pub(crate) use header::Header;
pub(crate) use instructions::{
    Call, CallOwnership, CalleeContract, Match, MatchCounts, MatchInstr, MatchPredicate, Opcode,
    PREDICATE_SLOTS, Return, SuccessorAddr, Switch, SwitchEntry,
};
pub(crate) use module::{DecodedCall, DecodedInstr, DecodedMatch, DecodedPredicate, Instruction};
pub(crate) use node_kind_constraint::NodeKindConstraint;
pub(crate) use plotnik_runtime::{Nav, deserialize_dfa};
pub(crate) use predicate_op::PredicateOp;
pub(crate) use render::ModuleRenderContext;
pub(crate) use sections::{FieldEntry, NodeKindEntry, SYMBOL_NAME_ENTRY_SIZE};
