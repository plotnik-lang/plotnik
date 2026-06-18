//! Bytecode format and runtime types for Plotnik.
//!
//! This crate contains:
//! - Bytecode format definitions (Module, Header, instructions)
//! - Type system primitives (TypeKind, Arity, QuantifierKind)
//! - Runtime helpers (Colors, PredicateOp, DFA deserialization)

#![allow(clippy::comparison_chain)]

pub mod bytecode;
pub mod dfa;
#[cfg(test)]
mod dfa_tests;
pub mod predicate_op;
pub mod type_system;

pub use bytecode::{
    AlignedVec, ByteStorage, Call, Effect, EffectKind, EncodeError, Entrypoint, EntrypointsView,
    FieldEntry, GrammarTableView, HEADER_SIZE, Header, Instruction, LineBuilder, MAGIC,
    MAX_MATCH_PAYLOAD_SLOTS, MAX_NEG_FIELDS, MAX_POST_EFFECTS, MAX_PRE_EFFECTS, MAX_SUCCESSORS,
    Match, MatchInstr, MatchPredicate, Module, ModuleError, Nav, NodeKindConstraint, NodeKindEntry,
    Opcode, PREAMBLE_NAME, REGEX_TABLE_ENTRY_SIZE, RegexView, Return, SECTION_ALIGN, STEP_SIZE,
    STRING_TABLE_ENTRY_SIZE, SectionOffsets, Slice, StepAddr, StepId, StringId, StringsView,
    Symbol, Trampoline, TypeDefKind, TypeDef, TypeId, TypeKind, TypeMember, TypeNameEntry,
    TypesView, VERSION, align_to_section, cols, dump, format_effect, nav_symbol,
    select_match_opcode, superscript, trace, truncate_text, width_for_count,
};
pub use dfa::{RegexDfas, deserialize_dfa};
pub use predicate_op::PredicateOp;
pub use type_system::{Arity, PrimitiveType, QuantifierKind};
