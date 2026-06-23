//! Bytecode format and runtime types for Plotnik.
//!
//! Public bytecode facade for compiled Plotnik queries.

#![allow(clippy::comparison_chain)]

mod bytecode;
mod dfa;
#[cfg(test)]
mod dfa_tests;
mod predicate_op;
pub mod type_system;

pub use bytecode::{EncodeError, Entrypoint, Module, ModuleError, TypeId, dump};
pub use type_system::{Arity, PrimitiveType, QuantifierKind, TypeKind};

pub(crate) use bytecode::{
    Call, Effect, EffectKind, EntrypointsView, FieldEntry, HEADER_SIZE, Header, Instruction,
    LineBuilder, MAX_MATCH_PAYLOAD_SLOTS, MAX_PRE_EFFECTS, Match, MatchInstr, MatchPredicate, Nav,
    NodeKindConstraint, NodeKindEntry, PREAMBLE_NAME, REGEX_TABLE_ENTRY_SIZE, Return,
    SECTION_ALIGN, STEP_SIZE, StepAddr, StepId, StringId, StringsView, Symbol, Trampoline, TypeDef,
    TypeDefKind, TypeMember, TypeNameEntry, TypesView, cols, format_effect, nav_symbol,
    select_match_opcode, trace, truncate_text, width_for_count,
};
pub(crate) use dfa::deserialize_dfa;
pub(crate) use predicate_op::PredicateOp;
