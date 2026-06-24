//! Bytecode emission.
//!
//! The pipeline runs as per-phase modules under `compiler::emit`; the data they
//! share lives in `tables`. This module is the driver that sequences them.

mod driver;
mod instructions;
mod layout;
mod module;
mod regex;
mod strings;
pub(in crate::compiler) mod tables;
mod types;

pub(in crate::compiler) use driver::{emit, emit_unchecked};

#[cfg(test)]
mod capacity_tests;
#[cfg(test)]
mod layout_tests;
