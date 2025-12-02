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

use crate::Result;
use crate::ast::lexer::lex;
use crate::ast::parser::{self, Parser};
use crate::ast::{Diagnostic, Parse, Root, SyntaxNode};
use named_defs::SymbolTable;
use shape_cardinalities::ShapeCardinality;

/// Builder for configuring and creating a [`Query`].
pub struct QueryBuilder<'a> {
    source: &'a str,
    #[cfg(debug_assertions)]
    debug_fuel: Option<Option<u32>>,
    exec_fuel: Option<Option<u32>>,
    recursion_fuel: Option<Option<u32>>,
}

impl<'a> QueryBuilder<'a> {
    /// Create a new builder for the given source.
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            #[cfg(debug_assertions)]
            debug_fuel: None,
            exec_fuel: None,
            recursion_fuel: None,
        }
    }

    /// Set debug fuel limit (debug builds only). None = infinite.
    ///
    /// Debug fuel resets on each token consumed. It detects parser bugs
    /// where no progress is made. Panics when exhausted.
    #[cfg(debug_assertions)]
    pub fn with_debug_fuel(mut self, limit: Option<u32>) -> Self {
        self.debug_fuel = Some(limit);
        self
    }

    /// Set execution fuel limit. None = infinite.
    ///
    /// Execution fuel never replenishes. It protects against large inputs.
    /// Returns error when exhausted.
    pub fn with_exec_fuel(mut self, limit: Option<u32>) -> Self {
        self.exec_fuel = Some(limit);
        self
    }

    /// Set recursion depth limit. None = infinite.
    ///
    /// Recursion fuel restores when exiting recursion. It protects against
    /// deeply nested input. Returns error when exhausted.
    pub fn with_recursion_fuel(mut self, limit: Option<u32>) -> Self {
        self.recursion_fuel = Some(limit);
        self
    }

    /// Build the query, running all analysis passes.
    ///
    /// Returns `Err` if fuel limits are exceeded.
    pub fn build(self) -> Result<Query<'a>> {
        let tokens = lex(self.source);
        let mut parser = Parser::new(self.source, tokens);

        #[cfg(debug_assertions)]
        if let Some(limit) = self.debug_fuel {
            parser = parser.with_debug_fuel(limit);
        }

        if let Some(limit) = self.exec_fuel {
            parser = parser.with_exec_fuel(limit);
        }

        if let Some(limit) = self.recursion_fuel {
            parser = parser.with_recursion_fuel(limit);
        }

        let parse = parser::parse_with_parser(parser)?;
        Ok(Query::from_parse(self.source, parse))
    }
}

/// A parsed and analyzed query.
///
/// Construction succeeds unless fuel limits are exceeded.
/// Check [`is_valid`](Self::is_valid) or [`errors`](Self::errors)
/// to determine if the query has syntax/semantic errors.
#[derive(Debug, Clone)]
pub struct Query<'a> {
    source: &'a str,
    parse: Parse,
    symbols: SymbolTable,
    errors: Vec<Diagnostic>,
    shape_cardinalities: HashMap<SyntaxNode, ShapeCardinality>,
}

impl<'a> Query<'a> {
    /// Parse and analyze a query from source text.
    ///
    /// Returns `Err` if fuel limits are exceeded.
    /// Syntax/semantic errors are collected and accessible via [`errors`](Self::errors).
    pub fn new(source: &'a str) -> Result<Self> {
        QueryBuilder::new(source).build()
    }

    /// Create a builder for configuring parser limits.
    pub fn builder(source: &'a str) -> QueryBuilder<'a> {
        QueryBuilder::new(source)
    }

    /// Internal: create Query from already-parsed input.
    fn from_parse(source: &'a str, parse: Parse) -> Self {
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

    #[allow(dead_code)]
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
        let q = Query::new("Expr = (expression)").unwrap();
        assert!(q.is_valid());
        assert!(q.symbols().get("Expr").is_some());
    }

    #[test]
    fn parse_error() {
        let q = Query::new("(unclosed").unwrap();
        assert!(!q.is_valid());
        assert!(q.dump_errors().contains("expected"));
    }

    #[test]
    fn resolution_error() {
        let q = Query::new("(call (Undefined))").unwrap();
        assert!(!q.is_valid());
        assert!(q.dump_errors().contains("undefined reference"));
    }

    #[test]
    fn combined_errors() {
        let q = Query::new("(call (Undefined) extra)").unwrap();
        assert!(!q.is_valid());
        assert!(!q.errors().is_empty());
    }
}
