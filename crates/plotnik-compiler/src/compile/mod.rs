//! Thompson-like NFA construction for query compilation.
//!
//! Compiles query AST expressions into bytecode IR with symbolic labels.
//! Labels are resolved to concrete StepIds during the layout phase.
//! A `MemberRef` carries a parent type plus relative index, resolved to an
//! absolute member index at emit time.
//!
//! # Module Organization
//!
//! The compiler is split into focused modules:
//! - `capture`: Capture effects handling (Node + Set)
//! - `expressions`: Leaf expression compilation (named/anon nodes, refs, fields, captures)
//! - `navigation`: Navigation mode computation for anchors and quantifiers
//! - `quantifier`: Unified quantifier compilation (*, +, ?)
//! - `scope`: Scope management for struct/array wrappers
//! - `sequences`: Sequence and alternation compilation

mod capture;
mod collapse_up;
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
pub mod verify;

#[cfg(test)]
mod capture_tests;
#[cfg(test)]
mod collapse_up_tests;
#[cfg(test)]
mod compile_tests;
#[cfg(test)]
mod lower_tests;
#[cfg(test)]
mod quantifier_tests;

pub use capture::CaptureEffects;
pub use collapse_up::collapse_up;
pub use compiler::{CompileCtx, Compiler};
pub use dce::remove_unreachable;
pub use epsilon_elim::eliminate_epsilons;
pub use error::CompileResult;
pub use lower::lower;
