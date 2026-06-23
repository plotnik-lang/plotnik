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

pub use bytecode::{
    Call, Effect, EffectKind, EncodeError, Entrypoint, Instruction, Match, MatchInstr,
    MatchPredicate, Module, ModuleError, Nav, NodeKindConstraint, Opcode, Return, StepAddr, StepId,
    StringId, Trampoline, TypeId, dump,
};
pub use predicate_op::PredicateOp;
pub use type_system::{Arity, PrimitiveType, QuantifierKind, TypeKind};

pub(crate) use bytecode::{
    EntrypointsView, FieldEntry, HEADER_SIZE, Header, LineBuilder, MAX_MATCH_PAYLOAD_SLOTS,
    MAX_PRE_EFFECTS, NodeKindEntry, PREAMBLE_NAME, REGEX_TABLE_ENTRY_SIZE, SECTION_ALIGN,
    STEP_SIZE, StringsView, Symbol, TypeDef, TypeDefKind, TypeMember, TypeNameEntry, TypesView,
    cols, format_effect, nav_symbol, select_match_opcode, trace, truncate_text, width_for_count,
};
pub(crate) use dfa::deserialize_dfa;
