//! Structural validation: anchors, predicates, alternative labeling, empty constructs.

pub(crate) mod anchor_context;
mod invariants;
pub mod validation;

#[cfg(test)]
mod anchor_context_tests;
