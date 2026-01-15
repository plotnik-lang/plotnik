//! Bytecode format and runtime types for Plotnik.
//!
//! This crate contains:
//! - Bytecode format definitions (Module, Header, instructions)
//! - Type system primitives (TypeKind, Arity, QuantifierKind)
//! - Runtime helpers (Colors, PredicateOp, DFA deserialization)

#![allow(clippy::comparison_chain)]

pub mod bytecode;
pub mod dfa;
pub mod predicate_op;
pub mod type_system;

// Re-export commonly used items at crate root
pub use bytecode::{
    AlignedVec, ByteStorage, Call, EffectOp, EffectOpcode, Entrypoint, EntrypointsView,
    FieldSymbol, Header, Instruction, LineBuilder, MAGIC, MAX_MATCH_PAYLOAD_SLOTS,
    MAX_PRE_EFFECTS, Match, Module, ModuleError, Nav, NodeSymbol, NodeTypeIR, Opcode, RegexView,
    Return, SECTION_ALIGN, STEP_SIZE, SectionOffsets, Slice, StepAddr, StepId, StringId,
    StringsView, Symbol, SymbolsView, Trampoline, TriviaEntry, TriviaView, TypeData, TypeDef,
    TypeId, TypeKind, TypeMember, TypeName, TypesView, VERSION, align_to_section, cols, dump,
    format_effect, nav_symbol, select_match_opcode, superscript, trace, truncate_text,
    width_for_count,
};
pub use dfa::deserialize_dfa;
pub use predicate_op::PredicateOp;
pub use type_system::{Arity, PrimitiveType, QuantifierKind};
