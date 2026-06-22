//! Semantic validation passes.
//!
//! Validates semantic constraints that aren't captured by parsing or type checking:
//! - Alternation kind consistency (alt_kinds)
//! - Anchor placement rules (anchors)
//! - Empty constructs (empty_constructs)
//! - Predicate regex patterns (predicates)

use indexmap::IndexMap;

use crate::SourceId;
use crate::diagnostics::Diagnostics;
use crate::parser::Root;
use crate::source::SourceMap;

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

/// Inputs for the whole AST validation stage.
pub struct AstValidationInput<'q, 'd> {
    pub source_map: &'q SourceMap,
    pub ast_map: &'q IndexMap<SourceId, Root>,
    pub diag: &'d mut Diagnostics,
}

pub use plotnik_compiler_core::ValidatedAst;

pub fn validate_ast<'q>(input: AstValidationInput<'q, '_>) -> ValidatedAst<'q> {
    for source in input.source_map.iter() {
        let ast = input
            .ast_map
            .get(&source.id)
            .expect("parsed source must have an AST");
        validate_alt_kinds(ValidationInput {
            source_id: source.id,
            ast,
            diag: &mut *input.diag,
        });
        validate_anchors(ValidationInput {
            source_id: source.id,
            ast,
            diag: &mut *input.diag,
        });
        validate_empty_constructs(ValidationInput {
            source_id: source.id,
            ast,
            diag: &mut *input.diag,
        });
        validate_predicates(PredicateInput {
            source_id: source.id,
            ast,
            source_content: source.content,
            diag: &mut *input.diag,
        });
    }

    ValidatedAst::new(input.source_map, input.ast_map)
}

pub use alt_kinds::validate_alt_kinds;
pub use anchors::validate_anchors;
pub use empty_constructs::validate_empty_constructs;
pub use predicates::validate_predicates;
