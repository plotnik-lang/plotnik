//! Bytecode module for compiled Plotnik queries.
//!
//! Implements the binary format specified in `docs/binary-format/`.

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
mod nav;
mod node_kind_constraint;
mod sections;
mod type_meta;

pub use constants::{
    HEADER_SIZE, MAGIC, MAX_MATCH_PAYLOAD_SLOTS, MAX_PRE_EFFECTS, REGEX_TABLE_ENTRY_SIZE,
    SECTION_ALIGN, STEP_SIZE, STRING_TABLE_ENTRY_SIZE, VERSION,
};

pub use ids::{StringId, TypeId};

pub use header::Header;

pub use sections::{FieldEntry, NodeKindEntry};

pub use entrypoint::Entrypoint;

pub use type_meta::{TypeDef, TypeDefKind, TypeMember, TypeNameEntry};

pub use nav::Nav;

pub use effects::{Effect, EffectKind};

pub use instructions::{
    Call, EncodeError, Match, MatchInstr, MatchPredicate, Opcode, Return, StepAddr, StepId,
    Trampoline, select_match_opcode,
};

pub use module::{EntrypointsView, Instruction, Module, ModuleError, StringsView, TypesView};

pub use dump::dump;

pub use format::{
    LineBuilder, PREAMBLE_NAME, Symbol, cols, format_effect, nav_symbol, trace, truncate_text,
    width_for_count,
};

pub use node_kind_constraint::NodeKindConstraint;

#[cfg(test)]
mod aligned_vec_tests;
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
mod type_meta_tests;
