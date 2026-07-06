//! Semantic validation passes.
//!
//! Validates semantic constraints that aren't captured by parsing or type checking:
//! - Alternation kind consistency (alt_kinds)
//! - Anchor placement rules (anchors)
//! - Empty constructs (empty_constructs)
//! - Predicate regex patterns (predicates)
//! - String escape sequences (strings)

use indexmap::IndexMap;

use crate::compiler::diagnostics::report::Diagnostics;
use crate::compiler::diagnostics::source::SourceId;
use crate::compiler::diagnostics::source::SourceMap;
use crate::compiler::parse::ast::Root;

pub mod alt_kinds;
pub mod anchors;
pub mod empty_constructs;
pub mod predicates;
pub mod strings;

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
pub struct ShapeValidationInput<'q, 'd> {
    pub source_map: &'q SourceMap,
    pub ast_map: &'q IndexMap<SourceId, Root>,
    pub diag: &'d mut Diagnostics,
}

/// AST bundle admitted past this validation boundary.
pub(crate) struct ValidatedAst<'q> {
    source_map: &'q SourceMap,
    ast_map: &'q IndexMap<SourceId, Root>,
}

impl<'q> ValidatedAst<'q> {
    fn new(source_map: &'q SourceMap, ast_map: &'q IndexMap<SourceId, Root>) -> Self {
        assert_eq!(
            source_map.len(),
            ast_map.len(),
            "validated AST must contain exactly one root per source",
        );
        assert!(
            source_map
                .iter()
                .all(|source| ast_map.contains_key(&source.id)),
            "validated AST must contain every source",
        );

        Self {
            source_map,
            ast_map,
        }
    }

    pub(crate) fn source_map(&self) -> &'q SourceMap {
        self.source_map
    }

    pub(crate) fn ast_map(&self) -> &'q IndexMap<SourceId, Root> {
        self.ast_map
    }
}

pub fn validate_ast<'q>(input: ShapeValidationInput<'q, '_>) -> Option<ValidatedAst<'q>> {
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
        strings::validate_strings(ValidationInput {
            source_id: source.id,
            ast,
            diag: &mut *input.diag,
        });
    }

    (!input.diag.has_errors()).then(|| ValidatedAst::new(input.source_map, input.ast_map))
}

pub use alt_kinds::validate_alt_kinds;
pub use anchors::validate_anchors;
pub use empty_constructs::validate_empty_constructs;
pub use predicates::validate_predicates;
