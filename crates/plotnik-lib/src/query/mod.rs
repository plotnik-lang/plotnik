//! Query processing: parsing, analysis, and validation pipeline.

mod dump;
mod invariants;
mod printer;
pub use printer::QueryPrinter;

pub mod alt_kinds;
pub mod recursion;
pub mod shapes;
pub mod symbol_table;

#[cfg(test)]
mod alt_kinds_tests;
#[cfg(test)]
mod mod_tests;
#[cfg(test)]
mod printer_tests;
#[cfg(test)]
mod recursion_tests;
#[cfg(test)]
mod shapes_tests;
#[cfg(test)]
mod symbol_table_tests;

use std::collections::HashMap;

use rowan::GreenNodeBuilder;

use crate::Result;
use crate::diagnostics::Diagnostics;
use crate::parser::cst::SyntaxKind;
use crate::parser::lexer::lex;
use crate::parser::{FuelState, ParseResult, Parser, Root, SyntaxNode, ast};
use shapes::ShapeCardinality;
use symbol_table::SymbolTable;

/// Builder for configuring and creating a [`Query`].
pub struct QueryBuilder<'a> {
    source: &'a str,
    exec_fuel: Option<u32>,
    recursion_fuel: Option<u32>,
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
        self.exec_fuel = limit;
        self
    }

    /// Set recursion depth limit. None = infinite.
    ///
    /// Recursion fuel restores when exiting recursion. It protects against
    /// deeply nested input. Returns error when exhausted.
    pub fn with_recursion_fuel(mut self, limit: Option<u32>) -> Self {
        self.recursion_fuel = limit;
        self
    }

    /// Build the query, running all analysis passes.
    ///
    /// Returns `Err` if fuel limits are exceeded.
    pub fn build(self) -> Result<Query<'a>> {
        let mut query = Query::empty(self.source);
        query.parse(self.exec_fuel, self.recursion_fuel)?;
        query.validate_alt_kinds();
        query.resolve_names();
        query.validate_recursion();
        query.infer_shapes();
        Ok(query)
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
    fuel_state: FuelState,
    // Diagnostics per pass
    parse_diagnostics: Diagnostics,
    alt_kind_diagnostics: Diagnostics,
    resolve_diagnostics: Diagnostics,
    recursion_diagnostics: Diagnostics,
    shapes_diagnostics: Diagnostics,
}

fn empty_root() -> Root {
    let mut builder = GreenNodeBuilder::new();
    builder.start_node(SyntaxKind::Root.into());
    builder.finish_node();
    let green = builder.finish();
    Root::cast(SyntaxNode::new_root(green)).expect("we just built a Root node")
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

    fn empty(source: &'a str) -> Self {
        Self {
            source,
            ast: empty_root(),
            symbol_table: SymbolTable::default(),
            shape_cardinality_table: HashMap::new(),
            fuel_state: FuelState::default(),
            parse_diagnostics: Diagnostics::new(),
            alt_kind_diagnostics: Diagnostics::new(),
            resolve_diagnostics: Diagnostics::new(),
            recursion_diagnostics: Diagnostics::new(),
            shapes_diagnostics: Diagnostics::new(),
        }
    }

    fn parse(&mut self, exec_fuel: Option<u32>, recursion_fuel: Option<u32>) -> Result<()> {
        let tokens = lex(self.source);
        let parser = Parser::new(self.source, tokens)
            .with_exec_fuel(exec_fuel)
            .with_recursion_fuel(recursion_fuel);

        let ParseResult {
            root,
            diagnostics,
            fuel_state,
        } = parser.parse()?;
        self.ast = root;
        self.parse_diagnostics = diagnostics;
        self.fuel_state = fuel_state;
        Ok(())
    }

    pub(crate) fn as_cst(&self) -> &SyntaxNode {
        self.ast.as_cst()
    }

    pub(crate) fn root(&self) -> &Root {
        &self.ast
    }

    pub(crate) fn shape_cardinality(&self, node: &SyntaxNode) -> ShapeCardinality {
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
    pub fn diagnostics(&self) -> Diagnostics {
        let mut all = Diagnostics::new();
        all.extend(self.parse_diagnostics.clone());
        all.extend(self.alt_kind_diagnostics.clone());
        all.extend(self.resolve_diagnostics.clone());
        all.extend(self.recursion_diagnostics.clone());
        all.extend(self.shapes_diagnostics.clone());
        all
    }

    /// Query is valid if there are no error-severity diagnostics (warnings are allowed).
    pub fn is_valid(&self) -> bool {
        !self.parse_diagnostics.has_errors()
            && !self.alt_kind_diagnostics.has_errors()
            && !self.resolve_diagnostics.has_errors()
            && !self.recursion_diagnostics.has_errors()
            && !self.shapes_diagnostics.has_errors()
    }
}
