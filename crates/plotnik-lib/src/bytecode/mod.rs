//! Bytecode module for compiled Plotnik queries.
//!
//! Implements the binary format specified in `docs/binary-format/`.

mod constants;
mod effects;
mod entrypoint;
mod header;
mod ids;
mod instructions;
mod nav;
mod sections;
mod type_meta;

pub use constants::{
    MAGIC, SECTION_ALIGN, STEP_ACCEPT, STEP_SIZE, TYPE_CUSTOM_START, TYPE_NODE, TYPE_STRING,
    TYPE_VOID, VERSION,
};

pub use ids::{QTypeId, StepId, StringId};

pub use header::Header;

pub use nav::Nav;

pub use sections::{FieldSymbol, NodeSymbol, Slice, TriviaEntry};

pub use effects::{EffectOp, EffectOpcode};

pub use entrypoint::Entrypoint;

pub use type_meta::{TypeDef, TypeKind, TypeMember, TypeMetaHeader, TypeName};

pub use instructions::{
    Call, Match, MatchView, Opcode, Return, align_to_section, select_match_opcode,
};

#[cfg(test)]
mod instructions_tests;
