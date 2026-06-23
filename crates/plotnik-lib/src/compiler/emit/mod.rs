//! Bytecode emission.
//!
//! The pipeline runs as per-phase modules under `compiler::emit`, each
//! depending only on `compiler::core`. This module is the driver that sequences
//! them.

mod driver;
mod instructions;
mod layout_pass;
mod module_pass;
mod regex;
mod strings;
mod types;

pub use driver::{emit, emit_unchecked};

pub use crate::compiler::core::{
    EmitError, EmitInput, RegexTableBuilder, StringTableBuilder, TypeTableBuilder,
};
pub use crate::compiler::emit::regex::deserialize_dfa;

pub mod layout {
    pub use crate::compiler::emit::layout_pass::CacheAligned;
}

#[cfg(test)]
mod capacity_tests;
#[cfg(test)]
mod layout_tests;
