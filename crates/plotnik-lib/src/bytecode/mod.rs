//! Bytecode module for compiled Plotnik queries.
//!
//! Implements the binary format specified in `docs/binary-format/`.

mod constants;
mod dump;
mod effects;
mod entrypoint;
mod format;
mod header;
mod ids;
mod instructions;
mod ir;
mod module;
mod nav;
mod sections;
mod type_meta;

pub use constants::{
    MAGIC, MAX_MATCH_PAYLOAD_SLOTS, NAMED_WILDCARD, SECTION_ALIGN, STEP_SIZE, VERSION,
};

pub use ids::{StringId, TypeId};

pub use header::{Header, flags};

pub use sections::{FieldSymbol, NodeSymbol, Slice, TriviaEntry};

pub use entrypoint::Entrypoint;

pub use type_meta::{TypeDef, TypeKind, TypeMember, TypeMetaHeader, TypeName};

pub use nav::Nav;

pub use effects::{EffectOp, EffectOpcode};

pub use instructions::{
    Call, Match, MatchView, Opcode, Return, StepAddr, StepId, Trampoline, align_to_section,
    select_match_opcode,
};

pub use module::{
    ByteStorage, EntrypointsView, Instruction, InstructionView, Module, ModuleError, StringsView,
    SymbolsView, TriviaView, TypesView,
};

pub use dump::dump;

pub use format::{
    LineBuilder, Symbol, cols, format_effect, nav_symbol, nav_symbol_epsilon, superscript, trace,
    truncate_text, width_for_count,
};

pub use ir::{
    CallIR, EffectIR, InstructionIR, Label, LayoutResult, MatchIR, MemberRef, ReturnIR,
    TrampolineIR,
};

#[cfg(test)]
mod instructions_tests;
#[cfg(test)]
mod ir_tests;
#[cfg(test)]
mod module_tests;
