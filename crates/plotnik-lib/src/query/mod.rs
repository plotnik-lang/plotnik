//! Query processing: parsing, analysis, and validation pipeline.

mod dump;
mod invariants;
mod printer;
pub use printer::QueryPrinter;

pub mod alt_kind;
pub mod ref_cycles;
pub mod shape_cardinalities;
pub mod symbol_table;

#[cfg(test)]
mod alt_kind_tests;
#[cfg(test)]
mod mod_tests;
#[cfg(test)]
mod printer_tests;
#[cfg(test)]
mod ref_cycles_tests;
#[cfg(test)]
mod shape_cardinalities_tests;
#[cfg(test)]
mod symbol_table_tests;

use std::collections::HashMap;

use crate::Result;
use crate::diagnostics::Diagnostics;
use crate::parser::cst::SyntaxKind;
use crate::parser::lexer::lex;
use crate::parser::{self, Parser, Root, SyntaxNode, ast};
use shape_cardinalities::ShapeCardinality;
use symbol_table::SymbolTable;

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

        let (parse, parse_diagnostics) = parser::parse_with_parser(parser)?;
        Ok(Query::from_parse(self.source, parse, parse_diagnostics))
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
    ast: Root,
    symbol_table: SymbolTable<'a>,
    shape_cardinality_table: HashMap<ast::Expr, ShapeCardinality>,
    // Diagnostics per pass
    parse_diagnostics: Diagnostics,
    alt_kind_diagnostics: Diagnostics,
    resolve_diagnostics: Diagnostics,
    ref_cycle_diagnostics: Diagnostics,
    shape_diagnostics: Diagnostics,
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

    fn from_parse(source: &'a str, ast: Root, parse_diagnostics: Diagnostics) -> Self {
        let mut query = Self {
            source,
            ast,
            symbol_table: SymbolTable::default(),
            shape_cardinality_table: HashMap::new(),
            parse_diagnostics,
            alt_kind_diagnostics: Diagnostics::new(),
            resolve_diagnostics: Diagnostics::new(),
            ref_cycle_diagnostics: Diagnostics::new(),
            shape_diagnostics: Diagnostics::new(),
        };

        query.validate_alt_kinds();
        query.resolve_symbols();
        query.validate_ref_cycles();
        query.analyze_shape_cardinalities();

        query
    }

    #[allow(dead_code)]
    pub fn source(&self) -> &str {
        self.source
    }

    pub fn as_cst(&self) -> &SyntaxNode {
        self.ast.as_cst()
    }

    pub fn root(&self) -> &Root {
        &self.ast
    }

    pub fn shape_cardinality(&self, node: &SyntaxNode) -> ShapeCardinality {
        // Error nodes are invalid
        if node.kind() == SyntaxKind::Error {
            return ShapeCardinality::Invalid;
        }

        // Root: cardinality based on definition count
        if let Some(root) = Root::cast(node.clone()) {
            return if root.defs().count() > 1 {
                ShapeCardinality::Many
            } else {
                ShapeCardinality::One
            };
        }

        // Def: delegate to body's cardinality
        if let Some(def) = ast::Def::cast(node.clone()) {
            return def
                .body()
                .and_then(|b| self.shape_cardinality_table.get(&b).copied())
                .unwrap_or(ShapeCardinality::Invalid);
        }

        // Branch: delegate to body's cardinality
        if let Some(branch) = ast::Branch::cast(node.clone()) {
            return branch
                .body()
                .and_then(|b| self.shape_cardinality_table.get(&b).copied())
                .unwrap_or(ShapeCardinality::Invalid);
        }

        // Expr: direct lookup
        ast::Expr::cast(node.clone())
            .and_then(|e| self.shape_cardinality_table.get(&e).copied())
            .unwrap_or(ShapeCardinality::One)
    }

    /// All diagnostics combined from all passes.
    pub fn all_diagnostics(&self) -> Diagnostics {
        let mut all = Diagnostics::new();
        all.extend(self.parse_diagnostics.clone());
        all.extend(self.alt_kind_diagnostics.clone());
        all.extend(self.resolve_diagnostics.clone());
        all.extend(self.ref_cycle_diagnostics.clone());
        all.extend(self.shape_diagnostics.clone());
        all
    }

    pub fn parse_diagnostics(&self) -> &Diagnostics {
        &self.parse_diagnostics
    }

    pub fn alt_kind_diagnostics(&self) -> &Diagnostics {
        &self.alt_kind_diagnostics
    }

    pub fn resolve_diagnostics(&self) -> &Diagnostics {
        &self.resolve_diagnostics
    }

    pub fn ref_cycle_diagnostics(&self) -> &Diagnostics {
        &self.ref_cycle_diagnostics
    }

    pub fn shape_diagnostics(&self) -> &Diagnostics {
        &self.shape_diagnostics
    }

    pub fn diagnostics(&self) -> Diagnostics {
        self.all_diagnostics()
    }

    /// Query is valid if there are no error-severity diagnostics (warnings are allowed).
    pub fn is_valid(&self) -> bool {
        !self.parse_diagnostics.has_errors()
            && !self.alt_kind_diagnostics.has_errors()
            && !self.resolve_diagnostics.has_errors()
            && !self.ref_cycle_diagnostics.has_errors()
            && !self.shape_diagnostics.has_errors()
    }

    pub fn render_diagnostics(&self) -> String {
        self.all_diagnostics().printer(self.source).render()
    }

    pub fn render_diagnostics_colored(&self, colored: bool) -> String {
        self.all_diagnostics()
            .printer(self.source)
            .colored(colored)
            .render()
    }
}
