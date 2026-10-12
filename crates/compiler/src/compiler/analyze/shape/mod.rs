//! Structural validation: anchors, predicates, alternative labeling, empty constructs.

pub(crate) mod anchor_context;
mod invariants;
mod pattern_facts;
mod root_extent;
pub mod validation;

pub(crate) use pattern_facts::PatternFacts;
pub use root_extent::RootExtent;
