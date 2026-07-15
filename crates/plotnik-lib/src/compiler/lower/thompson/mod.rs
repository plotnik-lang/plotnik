//! Thompson-like NFA construction for query compilation.
//!
//! Compiles query AST patterns into bytecode IR with symbolic labels.
//! Labels are resolved to concrete code addresses during the layout phase.
//! Structured-result effects carry canonical member IDs from the retained
//! result model.
//!
//! # Module Organization
//!
//! The compiler is split into focused modules:
//! - `capture`: capture effects handling (`Node` + `RecordSet`)
//! - `patterns`: Leaf pattern compilation (named/anon nodes, refs, fields, captures)
//! - `navigation`: Navigation mode computation for anchors and quantifiers
//! - `quantifier`: Unified quantifier compilation (*, +, ?)
//! - `scope`: Scope management for record/list wrappers
//! - `sequences`: Sequence and alternation compilation

mod alternation;
mod builder;
mod capture;
mod capture_type;
mod navigation;
mod nfa_emit;
mod patterns;
mod quantifier;
mod scope;
mod sequences;

#[cfg(test)]
mod capture_tests;
#[cfg(test)]
mod patterns_tests;
#[cfg(test)]
mod quantifier_tests;

pub(in crate::compiler::lower) use builder::NfaBuilder;
