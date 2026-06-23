#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Structural validation: anchors, predicates, alternation kinds, empty constructs.

mod invariants;
pub mod validation;

pub use validation::{
    validate_alt_kinds, validate_anchors, validate_empty_constructs, validate_predicates,
};
