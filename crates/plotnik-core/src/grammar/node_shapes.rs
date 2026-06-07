//! Derive node metadata from grammar.json.

use crate::NodeShape;

use super::json::GrammarError;
use super::types::Grammar;

impl Grammar {
    /// Generate the node metadata needed by Plotnik from grammar.json.
    pub fn try_node_shapes(&self) -> Result<Vec<NodeShape>, GrammarError> {
        super::tree_sitter::node_shapes_for_raw(self.raw()).map_err(GrammarError::Analysis)
    }

    /// Generate node metadata for grammars that have already been accepted as valid.
    pub fn node_shapes(&self) -> Vec<NodeShape> {
        self.try_node_shapes()
            .expect("grammar analysis should succeed for valid grammar.json")
    }
}
