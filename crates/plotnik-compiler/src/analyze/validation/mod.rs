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

/// Inputs for the AST-only validation passes (alt kinds, anchors, empty
/// constructs).
pub struct ValidationInput<'q, 'd> {
    pub source_id: SourceId,
    pub ast: &'q Root,
    pub diag: &'d mut Diagnostics,
}

/// Inputs for predicate validation, which also needs the source text to slice
/// out and check the regex patterns embedded in predicates.
pub struct PredicateInput<'q, 'd> {
    pub source_id: SourceId,
    pub ast: &'q Root,
    pub source_content: &'q str,
    pub diag: &'d mut Diagnostics,
}

pub use alt_kinds::validate_alt_kinds;
pub use anchors::validate_anchors;
pub use empty_constructs::validate_empty_constructs;
pub use predicates::validate_predicates;
