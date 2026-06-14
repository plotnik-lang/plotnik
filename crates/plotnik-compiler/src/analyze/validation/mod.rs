//! Semantic validation passes.
//!
//! Validates semantic constraints that aren't captured by parsing or type checking:
//! - Alternation kind consistency (alt_kinds)
//! - Anchor placement rules (anchors)
//! - Empty constructs (empty_constructs)
//! - Predicate regex patterns (predicates)

use crate::SourceId;
use crate::diagnostics::Diagnostics;
use crate::parser::Root;

pub mod alt_kinds;
pub mod anchors;
pub mod empty_constructs;
pub mod predicates;

/// Shared inputs for the per-source validation passes.
///
/// `source_content` is only needed by passes that slice token text (predicates);
/// the rest operate on the AST alone.
pub struct ValidateInput<'q, 'd> {
    pub source_id: SourceId,
    pub ast: &'q Root,
    pub source_content: Option<&'q str>,
    pub diag: &'d mut Diagnostics,
}

#[cfg(test)]
mod alt_kinds_tests;
#[cfg(test)]
mod anchors_tests;
#[cfg(test)]
mod empty_constructs_tests;
#[cfg(test)]
mod predicates_tests;

pub use alt_kinds::validate_alt_kinds;
pub use anchors::validate_anchors;
pub use empty_constructs::validate_empty_constructs;
pub use predicates::validate_predicates;
