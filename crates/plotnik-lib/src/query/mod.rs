//! Query processing: parsing, analysis, and validation pipeline.

mod dump;
mod errors;
mod printer;
pub use printer::QueryPrinter;

pub mod alt_kind;
pub mod named_defs;
pub mod ref_cycles;
pub mod shape_cardinalities;

#[cfg(test)]
mod shape_cardinalities_tests;

use std::collections::HashMap;

use crate::ast::{self, Parse, Root, SyntaxError, SyntaxNode};
use named_defs::SymbolTable;
use shape_cardinalities::ShapeCardinality;

/// A parsed and analyzed query.
///
/// Construction always succeeds. Check [`is_valid`](Self::is_valid) or
/// [`errors`](Self::errors) to determine if the query is usable.
#[derive(Debug, Clone)]
pub struct Query<'a> {
    source: &'a str,
    parse: Parse,
    symbols: SymbolTable,
    errors: Vec<SyntaxError>,
    shape_cardinalities: HashMap<SyntaxNode, ShapeCardinality>,
}

impl<'a> Query<'a> {
    /// Parse and analyze a query from source text.
    ///
    /// This never fails. Errors are collected and accessible via [`errors`](Self::errors).
    pub fn new(source: &'a str) -> Self {
        let parse = ast::parse(source);
        let root = Root::cast(parse.syntax()).expect("parser always produces Root");

        let mut errors = parse.errors().to_vec();

        let alt_kind_errors = alt_kind::validate(&root);
        errors.extend(alt_kind_errors);

        let resolve_result = named_defs::resolve(&root);
        errors.extend(resolve_result.errors);

        let ref_cycle_errors = ref_cycles::validate(&root, &resolve_result.symbols);
        errors.extend(ref_cycle_errors);

        let shape_cardinalities = if errors.is_empty() {
            let cards = shape_cardinalities::infer(&root, &resolve_result.symbols);
            let shape_errors =
                shape_cardinalities::validate(&root, &resolve_result.symbols, &cards);
            errors.extend(shape_errors);
            cards
        } else {
            HashMap::new()
        };

        Self {
            source,
            parse,
            symbols: resolve_result.symbols,
            errors,
            shape_cardinalities,
        }
    }

    pub fn source(&self) -> &str {
        self.source
    }

    pub fn syntax(&self) -> SyntaxNode {
        self.parse.syntax()
    }

    pub fn root(&self) -> Root {
        Root::cast(self.parse.syntax()).expect("parser always produces Root")
    }

    pub fn symbols(&self) -> &SymbolTable {
        &self.symbols
    }

    pub fn shape_cardinality(&self, node: &SyntaxNode) -> ShapeCardinality {
        self.shape_cardinalities
            .get(node)
            .copied()
            .unwrap_or(ShapeCardinality::One)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_query() {
        let q = Query::new("Expr = (expression)");
        assert!(q.is_valid());
        assert!(q.symbols().get("Expr").is_some());
    }

    #[test]
    fn parse_error() {
        let q = Query::new("(unclosed");
        assert!(!q.is_valid());
        assert!(q.dump_errors().contains("expected"));
    }

    #[test]
    fn resolution_error() {
        let q = Query::new("(call (Undefined))");
        assert!(!q.is_valid());
        assert!(q.dump_errors().contains("undefined reference"));
    }

    #[test]
    fn combined_errors() {
        let q = Query::new("(call (Undefined) extra)");
        assert!(!q.is_valid());
        assert!(!q.errors().is_empty());
    }
}
