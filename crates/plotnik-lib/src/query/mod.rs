//! Query processing: parsing, analysis, and validation pipeline.

mod dump;
mod errors;
mod invariants;
mod printer;
pub use printer::QueryPrinter;

pub mod alt_kind;
pub mod named_defs;
pub mod ref_cycles;
pub mod shape_cardinalities;

#[cfg(test)]
mod alt_kind_tests;
#[cfg(test)]
mod mod_tests;
#[cfg(test)]
mod named_defs_tests;
#[cfg(test)]
mod printer_tests;
#[cfg(test)]
mod ref_cycles_tests;
#[cfg(test)]
mod shape_cardinalities_tests;

use std::collections::HashMap;

use crate::Result;
use crate::diagnostics::Diagnostics;
use crate::parser::lexer::lex;
use crate::parser::{self, Parse, Parser, Root, SyntaxNode};
use named_defs::SymbolTable;
use shape_cardinalities::ShapeCardinality;

/// Builder for configuring and creating a [`Query`].
pub struct QueryBuilder<'a> {
    source: &'a str,
    exec_fuel: Option<Option<u32>>,
    recursion_fuel: Option<Option<u32>>,
}

impl<'a> QueryBuilder<'a> {
    /// Create a new builder for the given source.
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            exec_fuel: None,
            recursion_fuel: None,
        }
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
/// Check [`is_valid`](Self::is_valid) or [`diagnostics`](Self::diagnostics)
/// to determine if the query has syntax/semantic issues.
#[derive(Debug, Clone)]
pub struct Query<'a> {
    source: &'a str,
    parse: Parse,
    symbols: SymbolTable,
    diagnostics: Diagnostics,
    shape_cardinalities: HashMap<SyntaxNode, ShapeCardinality>,
}

impl<'a> Query<'a> {
    /// Parse and analyze a query from source text.
    ///
    /// Returns `Err` if fuel limits are exceeded.
    /// Syntax/semantic diagnostics are collected and accessible via [`diagnostics`](Self::diagnostics).
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

        let mut diagnostics = parse.diagnostics().clone();

        let alt_kind_diags = alt_kind::validate(&root);
        diagnostics.extend(alt_kind_diags);

        let resolve_result = named_defs::resolve(&root);
        diagnostics.extend(resolve_result.diagnostics);

        let ref_cycle_diags = ref_cycles::validate(&root, &resolve_result.symbols);
        diagnostics.extend(ref_cycle_diags);

        let shape_cardinalities = if diagnostics.is_empty() {
            let cards = shape_cardinalities::infer(&root, &resolve_result.symbols);
            let shape_diags = shape_cardinalities::validate(&root, &resolve_result.symbols, &cards);
            diagnostics.extend(shape_diags);
            cards
        } else {
            HashMap::new()
        };

        Self {
            source,
            parse,
            symbols: resolve_result.symbols,
            diagnostics,
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
