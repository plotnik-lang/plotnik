//! Shared text-emission primitives for every generated-code backend.
//!
//! Backends still own their syntax and structural templates. This module owns
//! the mechanics that should not be reimplemented per language: deterministic
//! template substitution, indentation, semantic text ranges, identifier
//! policies, and literal formatting.

pub(crate) mod lits;
pub(crate) mod names;
pub(crate) mod sink;
pub(crate) mod template;

#[cfg(test)]
mod lits_tests;
#[cfg(test)]
mod names_tests;
#[cfg(test)]
mod sink_tests;
#[cfg(test)]
mod template_tests;
