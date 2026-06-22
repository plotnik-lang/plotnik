//! The validated-AST bundle admitted past the semantic validation boundary.

use indexmap::IndexMap;

use crate::{Root, SourceId, SourceMap};

/// Validated AST bundle admitted past the semantic validation boundary.
pub struct ValidatedAst<'q> {
    source_map: &'q SourceMap,
    ast_map: &'q IndexMap<SourceId, Root>,
}

impl<'q> ValidatedAst<'q> {
    pub fn new(source_map: &'q SourceMap, ast_map: &'q IndexMap<SourceId, Root>) -> Self {
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
