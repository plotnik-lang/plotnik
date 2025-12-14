//! Query processing pipeline.
//!
//! Stages: parse → alt_kinds → symbol_table → recursion → shapes → [qis → build_graph].
//! Each stage populates its own diagnostics. Use `is_valid()` to check
//! if any stage produced errors.
//!
//! The `build_graph` stage is optional and constructs the transition graph
//! for compilation to binary IR. QIS detection runs as part of this stage.

mod dump;
mod graph_qis;
mod invariants;
mod printer;
pub use printer::QueryPrinter;

pub mod alt_kinds;
pub mod graph;
mod graph_build;
mod graph_dump;
mod graph_optimize;
pub mod infer;
mod infer_dump;
#[cfg(feature = "plotnik-langs")]
pub mod link;
pub mod recursion;
pub mod shapes;
pub mod symbol_table;

pub use graph::{BuildEffect, BuildGraph, BuildMatcher, BuildNode, Fragment, NodeId, RefMarker};
pub use graph_optimize::OptimizeStats;
pub use infer::{
    InferredMember, InferredTypeDef, TypeDescription, TypeInferenceResult, UnificationError,
};
pub use symbol_table::UNNAMED_DEF;

#[cfg(test)]
mod alt_kinds_tests;
#[cfg(test)]
mod graph_build_tests;
#[cfg(test)]
mod graph_master_test;
#[cfg(test)]
mod graph_qis_tests;
#[cfg(test)]
mod infer_tests;
#[cfg(all(test, feature = "plotnik-langs"))]
mod link_tests;
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

use std::collections::{HashMap, HashSet};

#[cfg(feature = "plotnik-langs")]
use plotnik_langs::{NodeFieldId, NodeTypeId};

use rowan::GreenNodeBuilder;

use crate::Result;
use crate::diagnostics::Diagnostics;
use crate::parser::cst::SyntaxKind;
use crate::parser::lexer::lex;
use crate::parser::{ParseResult, Parser, Root, SyntaxNode, ast};

const DEFAULT_EXEC_FUEL: u32 = 1_000_000;
const DEFAULT_RECURSION_FUEL: u32 = 4096;

use shapes::ShapeCardinality;
use symbol_table::SymbolTable;

/// A parsed and analyzed query.
///
/// Create with [`new`](Self::new), optionally configure fuel limits,
/// then call [`exec`](Self::exec) to run analysis.
///
/// For compilation, call [`build_graph`](Self::build_graph) after `exec`.
///
/// Check [`is_valid`](Self::is_valid) or [`diagnostics`](Self::diagnostics)
/// to determine if the query has syntax/semantic issues.
/// Quantifier-Induced Scope trigger info.
///
/// When a quantified expression has ≥2 propagating captures, QIS creates
/// an implicit object scope so captures stay coupled per-iteration.
#[derive(Debug, Clone)]
pub struct QisTrigger<'a> {
    /// Capture names that propagate from this quantified expression.
    pub captures: Vec<&'a str>,
}

#[derive(Debug)]
pub struct Query<'a> {
    source: &'a str,
    ast: Root,
    symbol_table: SymbolTable<'a>,
    shape_cardinality_table: HashMap<ast::Expr, ShapeCardinality>,
    #[cfg(feature = "plotnik-langs")]
    node_type_ids: HashMap<&'a str, Option<NodeTypeId>>,
    #[cfg(feature = "plotnik-langs")]
    node_field_ids: HashMap<&'a str, Option<NodeFieldId>>,
    exec_fuel: Option<u32>,
    recursion_fuel: Option<u32>,
    exec_fuel_consumed: u32,
    parse_diagnostics: Diagnostics,
    alt_kind_diagnostics: Diagnostics,
    resolve_diagnostics: Diagnostics,
    recursion_diagnostics: Diagnostics,
    shapes_diagnostics: Diagnostics,
    #[cfg(feature = "plotnik-langs")]
    link_diagnostics: Diagnostics,
    // Graph compilation fields
    graph: BuildGraph<'a>,
    dead_nodes: HashSet<NodeId>,
    type_info: TypeInferenceResult<'a>,
    /// QIS triggers: quantified expressions with ≥2 propagating captures.
    qis_triggers: HashMap<ast::QuantifiedExpr, QisTrigger<'a>>,
    /// Counter for generating unique ref IDs during graph construction.
    next_ref_id: u32,
}

fn empty_root() -> Root {
    let mut builder = GreenNodeBuilder::new();
    builder.start_node(SyntaxKind::Root.into());
    builder.finish_node();
    let green = builder.finish();
    Root::cast(SyntaxNode::new_root(green)).expect("we just built a Root node")
}

impl<'a> Query<'a> {
    /// Create a new query from source text.
    ///
    /// Call [`exec`](Self::exec) to run analysis passes.
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            ast: empty_root(),
            symbol_table: SymbolTable::default(),
            shape_cardinality_table: HashMap::new(),
            #[cfg(feature = "plotnik-langs")]
            node_type_ids: HashMap::new(),
            #[cfg(feature = "plotnik-langs")]
            node_field_ids: HashMap::new(),
            exec_fuel: Some(DEFAULT_EXEC_FUEL),
            recursion_fuel: Some(DEFAULT_RECURSION_FUEL),
            exec_fuel_consumed: 0,
            parse_diagnostics: Diagnostics::new(),
            alt_kind_diagnostics: Diagnostics::new(),
            resolve_diagnostics: Diagnostics::new(),
            recursion_diagnostics: Diagnostics::new(),
            shapes_diagnostics: Diagnostics::new(),
            #[cfg(feature = "plotnik-langs")]
            link_diagnostics: Diagnostics::new(),
            graph: BuildGraph::default(),
            dead_nodes: HashSet::new(),
            type_info: TypeInferenceResult::default(),
            qis_triggers: HashMap::new(),
            next_ref_id: 0,
        }
    }

    /// Set execution fuel limit. None = infinite.
    ///
    /// Execution fuel never replenishes. It protects against large inputs.
    /// Returns error from [`exec`](Self::exec) when exhausted.
    pub fn with_exec_fuel(mut self, limit: Option<u32>) -> Self {
        self.exec_fuel = limit;
        self
    }

    /// Set recursion depth limit. None = infinite.
    ///
    /// Recursion fuel restores when exiting recursion. It protects against
    /// deeply nested input. Returns error from [`exec`](Self::exec) when exhausted.
    pub fn with_recursion_fuel(mut self, limit: Option<u32>) -> Self {
        self.recursion_fuel = limit;
        self
    }

    /// Run all analysis passes.
    ///
    /// Returns `Err` if fuel limits are exceeded.
    /// Syntax/semantic diagnostics are collected and accessible via [`diagnostics`](Self::diagnostics).
    pub fn exec(mut self) -> Result<Self> {
        self.try_parse()?;
        self.validate_alt_kinds();
        self.resolve_names();
        self.validate_recursion();
        self.infer_shapes();
        Ok(self)
    }

    /// Build the transition graph for compilation.
    ///
    /// This is an optional step after `exec`. It detects QIS triggers,
    /// constructs the graph, runs epsilon elimination, and infers types.
    ///
    /// Only runs if the query is valid (no errors from previous passes).
    pub fn build_graph(mut self) -> Self {
        if !self.is_valid() {
            return self;
        }
        self.detect_qis();
        self.construct_graph();
        self.infer_types(); // Run before optimization to avoid merged effects
        self.optimize_graph();
        self
    }

    /// Build graph and return dump of graph before optimization (for debugging).
    pub fn build_graph_with_pre_opt_dump(mut self) -> (Self, String) {
        if !self.is_valid() {
            return (self, String::new());
        }
        self.detect_qis();
        self.construct_graph();
        let pre_opt_dump = self.graph.dump();
        self.infer_types();
        self.optimize_graph();
        (self, pre_opt_dump)
    }

    fn try_parse(&mut self) -> Result<()> {
        let tokens = lex(self.source);
        let parser = Parser::new(self.source, tokens)
            .with_exec_fuel(self.exec_fuel)
            .with_recursion_fuel(self.recursion_fuel);

        let ParseResult {
            root,
            diagnostics,
            exec_fuel_consumed,
        } = parser.parse()?;
        self.ast = root;
        self.parse_diagnostics = diagnostics;
        self.exec_fuel_consumed = exec_fuel_consumed;
        Ok(())
    }

    pub(crate) fn as_cst(&self) -> &SyntaxNode {
        self.ast.as_cst()
    }

    pub(crate) fn root(&self) -> &Root {
        &self.ast
    }

    /// Access the constructed graph.
    pub fn graph(&self) -> &BuildGraph<'a> {
        &self.graph
    }

    /// Access the set of dead nodes (eliminated by optimization).
    pub fn dead_nodes(&self) -> &HashSet<NodeId> {
        &self.dead_nodes
    }

    /// Access the type inference result.
    pub fn type_info(&self) -> &TypeInferenceResult<'a> {
        &self.type_info
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

    /// All diagnostics combined from all passes (unfiltered).
    ///
    /// Use this for debugging or when you need to see all diagnostics
    /// including cascading errors.
    pub fn diagnostics_raw(&self) -> Diagnostics {
        let mut all = Diagnostics::new();
        all.extend(self.parse_diagnostics.clone());
        all.extend(self.alt_kind_diagnostics.clone());
        all.extend(self.resolve_diagnostics.clone());
        all.extend(self.recursion_diagnostics.clone());
        all.extend(self.shapes_diagnostics.clone());
        #[cfg(feature = "plotnik-langs")]
        all.extend(self.link_diagnostics.clone());
        all.extend(self.type_info.diagnostics.clone());
        all
    }

    /// All diagnostics combined from all passes.
    ///
    /// Returns diagnostics with cascading errors suppressed.
    /// For raw access, use [`diagnostics_raw`](Self::diagnostics_raw).
    pub fn diagnostics(&self) -> Diagnostics {
        self.diagnostics_raw()
    }

    /// Query is valid if there are no error-severity diagnostics (warnings are allowed).
    #[cfg(feature = "plotnik-langs")]
    pub fn is_valid(&self) -> bool {
        !self.parse_diagnostics.has_errors()
            && !self.alt_kind_diagnostics.has_errors()
            && !self.resolve_diagnostics.has_errors()
            && !self.recursion_diagnostics.has_errors()
            && !self.shapes_diagnostics.has_errors()
            && !self.link_diagnostics.has_errors()
    }

    /// Query is valid if there are no error-severity diagnostics (warnings are allowed).
    #[cfg(not(feature = "plotnik-langs"))]
    pub fn is_valid(&self) -> bool {
        !self.parse_diagnostics.has_errors()
            && !self.alt_kind_diagnostics.has_errors()
            && !self.resolve_diagnostics.has_errors()
            && !self.recursion_diagnostics.has_errors()
            && !self.shapes_diagnostics.has_errors()
    }

    /// Check if graph compilation produced type errors.
    pub fn has_type_errors(&self) -> bool {
        self.type_info.has_errors()
    }
}

impl<'a> TryFrom<&'a str> for Query<'a> {
    type Error = crate::Error;

    fn try_from(source: &'a str) -> Result<Self> {
        Self::new(source).exec()
    }
}

impl<'a> TryFrom<&'a String> for Query<'a> {
    type Error = crate::Error;

    fn try_from(source: &'a String) -> Result<Self> {
        Self::new(source.as_str()).exec()
    }
}
