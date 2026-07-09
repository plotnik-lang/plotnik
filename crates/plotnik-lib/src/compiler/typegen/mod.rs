#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Type declaration generation for target languages.
//!
//! TypeScript renders `.d.ts` declarations from a compiled bytecode module;
//! Rust renders output structs/enums for the proc-macro backend from the
//! analysis-level type model.
//!
//! # Example
//!
//! ```ignore
//! use plotnik_lib::typegen::typescript;
//! use plotnik_lib::bytecode::Module;
//!
//! let module = Module::load(&bytecode)?;
//! let output = typescript::emit(&module);
//! ```

pub mod rust;
pub mod typescript;
