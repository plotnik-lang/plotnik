//! Thompson-like NFA construction for query compilation.
//!
//! Compiles query AST patterns into bytecode IR with symbolic labels.
//! Labels are resolved to concrete StepIds during the layout phase.
//! A `MemberRef` carries a parent type plus relative index, resolved to an
//! absolute member index at emit time.
//!
//! # Module Organization
//!
//! The compiler is split into focused modules:
//! - `capture`: Capture effects handling (Node + Set)
//! - `patterns`: Leaf pattern compilation (named/anon nodes, refs, fields, captures)
//! - `navigation`: Navigation mode computation for anchors and quantifiers
//! - `quantifier`: Unified quantifier compilation (*, +, ?)
//! - `scope`: Scope management for struct/array wrappers
//! - `sequences`: Sequence and alternation compilation

mod alternation;
mod builder;
mod capture;
mod navigation;
mod nfa_emit;
mod patterns;
mod quantifier;
mod scalar;
mod scope;
mod sequences;

#[cfg(test)]
mod capture_tests;
#[cfg(test)]
mod patterns_tests;
#[cfg(test)]
mod quantifier_tests;

pub(in crate::compiler::lower) use builder::NfaBuilder;
