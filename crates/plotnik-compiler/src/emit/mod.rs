//! Bytecode emission.
//!
//! The pipeline runs as per-phase passes in the `plotnik-compiler-emit-*`
//! crates, each depending only on `plotnik-compiler-core`. This module is the
//! driver that sequences them, plus the historical `plotnik_compiler::emit`
//! facade for downstream callers.

mod driver;

pub use driver::{emit, emit_unchecked};

pub use plotnik_compiler_core::{
    EmitError, EmitInput, RegexTableBuilder, StringTableBuilder, TypeTableBuilder,
};
pub use plotnik_compiler_emit_regex::deserialize_dfa;

pub mod layout {
    pub use plotnik_compiler_emit_layout::CacheAligned;
}

#[cfg(test)]
mod capacity_tests;
#[cfg(test)]
mod layout_tests;
