//! Semantic validation passes.
//!
//! Validates semantic constraints that aren't captured by parsing or type checking:
//! - Alternation kind consistency (alt_kinds)
//! - Anchor placement rules (anchors)
//! - Empty constructs (empty_constructs)

pub mod alt_kinds;
pub mod anchors;
pub mod empty_constructs;

#[cfg(test)]
mod alt_kinds_tests;
#[cfg(test)]
mod anchors_tests;
#[cfg(test)]
mod empty_constructs_tests;

pub use alt_kinds::validate_alt_kinds;
pub use anchors::validate_anchors;
pub use empty_constructs::validate_empty_constructs;
