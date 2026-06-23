//! The validated-AST bundle admitted past the semantic validation boundary.

use indexmap::IndexMap;

use crate::compiler::core::{Root, SourceId, SourceMap};

/// Validated AST bundle admitted past the semantic validation boundary.
pub struct ValidatedAst<'q> {
    source_map: &'q SourceMap,
    ast_map: &'q IndexMap<SourceId, Root>,
}

impl<'q> ValidatedAst<'q> {
    pub(in crate::compiler) fn new(
        source_map: &'q SourceMap,
        ast_map: &'q IndexMap<SourceId, Root>,
    ) -> Self {
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

    pub fn source_map(&self) -> &'q SourceMap {
        self.source_map
    }

    pub fn ast_map(&self) -> &'q IndexMap<SourceId, Root> {
        self.ast_map
    }
}
