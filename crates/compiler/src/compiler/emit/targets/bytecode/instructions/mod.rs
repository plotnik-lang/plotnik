#![allow(clippy::module_inception)]

//! Instruction-encoding emission phase: resolve each instruction into its
//! instruction bytes.

mod instructions;

pub use instructions::emit_instructions;
