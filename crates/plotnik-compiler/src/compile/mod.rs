//! Thompson-like NFA construction for query compilation.
//!
//! Compiles query AST expressions into bytecode IR with symbolic labels.
//! Labels are resolved to concrete StepIds during the layout phase.
//! Member indices use deferred resolution via MemberRef for correct absolute indices.
//!
//! # Module Organization
//!
//! The compiler is split into focused modules:
//! - `capture`: Capture effects handling (Node/Text + Set)
//! - `expressions`: Leaf expression compilation (named/anon nodes, refs, fields, captures)
//! - `navigation`: Navigation mode computation for anchors and quantifiers
//! - `quantifier`: Unified quantifier compilation (*, +, ?)
//! - `scope`: Scope management for struct/array wrappers
//! - `sequences`: Sequence and alternation compilation

mod capture;
mod compiler;
mod dce;
pub(crate) mod epsilon_elim;
mod error;
mod expressions;
mod lower;
mod navigation;
mod quantifier;
mod scope;
mod sequences;
mod verify;

#[cfg(test)]
mod capture_tests;
#[cfg(test)]
mod compile_tests;
#[cfg(test)]
mod lower_tests;

pub use capture::CaptureEffects;
pub use compiler::{CompileCtx, Compiler};
pub use error::{CompileError, CompileResult};
