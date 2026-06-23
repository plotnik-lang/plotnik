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

pub use aligned_vec::AlignedVec;

pub use constants::{
    HEADER_SIZE, MAGIC, MAX_MATCH_PAYLOAD_SLOTS, MAX_NEG_FIELDS, MAX_POST_EFFECTS, MAX_PRE_EFFECTS,
    MAX_SUCCESSORS, REGEX_TABLE_ENTRY_SIZE, SECTION_ALIGN, STEP_SIZE, STRING_TABLE_ENTRY_SIZE,
    VERSION,
};

pub use ids::{StringId, TypeId};

pub use header::{Header, SectionOffsets};

pub use sections::{FieldEntry, NodeKindEntry, Slice};

pub use entrypoint::Entrypoint;

pub use type_meta::{TypeDef, TypeDefKind, TypeKind, TypeMember, TypeNameEntry};

pub use nav::Nav;

pub use effects::{EFFECT_PAYLOAD_BITS, EFFECT_PAYLOAD_MAX, Effect, EffectKind};

pub use instructions::{
    Call, EncodeError, Match, MatchInstr, MatchPredicate, Opcode, Return, StepAddr, StepId,
    Trampoline, align_to_section, select_match_opcode,
};

pub use module::{
    ByteStorage, EntrypointsView, GrammarTableView, Instruction, Module, ModuleError, RegexView,
    StringsView, TypesView,
};

pub use dump::dump;

pub use format::{
    LineBuilder, PREAMBLE_NAME, Symbol, cols, format_effect, nav_symbol, superscript, trace,
    truncate_text, width_for_count,
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
mod sections_tests;
#[cfg(test)]
mod type_meta_tests;
