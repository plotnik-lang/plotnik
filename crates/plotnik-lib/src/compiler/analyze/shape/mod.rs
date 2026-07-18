//! Structural validation: anchors, predicates, alternative labeling, empty constructs.

pub(crate) mod anchor_context;
mod definition_facts;
mod invariants;
mod root_extent;
pub mod validation;

pub(crate) use definition_facts::DefinitionFacts;
pub use root_extent::RootExtent;
