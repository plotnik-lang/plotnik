//! Bytecode emission.
//!
//! The pipeline runs as per-phase modules under `compiler::emit`, each
//! depending only on `compiler::core`. This module is the driver that sequences
//! them.

mod driver;
mod instructions;
mod layout;
mod module;
mod regex;
mod strings;
mod types;

pub(in crate::compiler) use driver::{emit, emit_unchecked};

#[cfg(test)]
mod capacity_tests;
#[cfg(test)]
mod layout_tests;
