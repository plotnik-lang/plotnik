//! Recursion validation for definitions.
//!
//! Validates that recursive definitions are well-formed:
//! - Escapable: at least one non-recursive path exists
//! - Guarded: every recursive cycle consumes input

use indexmap::{IndexMap, IndexSet};
use rowan::TextRange;

use super::dependencies::{DependencyAnalysis, collect_refs};
use super::symbol_table::SymbolTable;
use super::visitor::{Visitor, walk_expr, walk_named_node};
use crate::Diagnostics;
use crate::diagnostics::DiagnosticKind;
use crate::parser::{AnonymousNode, Def, Expr, NamedNode, Ref, Root, SeqExpr};
use crate::query::SourceId;

/// Validate recursion using the pre-computed dependency analysis.
pub fn validate_recursion(
    analysis: &DependencyAnalysis,
    ast_map: &IndexMap<SourceId, Root>,
    symbol_table: &SymbolTable,
    diag: &mut Diagnostics,
) {
    let mut validator = RecursionValidator {
        ast_map,
        symbol_table,
        diag,
    };
    validator.validate(&analysis.sccs);
}

struct RecursionValidator<'a, 'd> {
    ast_map: &'a IndexMap<SourceId, Root>,
    symbol_table: &'a SymbolTable,
    diag: &'d mut Diagnostics,
}

impl<'a, 'd> RecursionValidator<'a, 'd> {
    fn validate(&mut self, sccs: &[Vec<String>]) {
        for scc in sccs {
            self.validate_scc(scc);
        }
    }

    fn validate_scc(&mut self, scc: &[String]) {
        // Filter out trivial non-recursive components.
        // A component is recursive if it has >1 node, or 1 node that references itself.
        if scc.len() == 1 {
            let name = &scc[0];
            let body = self
                .symbol_table
                .get(name)
                .expect("node in SCC must exist in symbol table");
            if !collect_refs(body, self.symbol_table).contains(name.as_str()) {
                return;
            }
        }

        let scc_set: IndexSet<&str> = scc.iter().map(String::as_str).collect();

        // 1. Check for infinite tree structure (Escape Analysis)
        // A valid recursive definition must have a non-recursive path.
        // If NO definition in the SCC has an escape path, the whole group is invalid.
        let has_escape = scc
            .iter()
            .filter_map(|name| self.symbol_table.get(name))
            .any(|body| expr_has_escape(body, &scc_set));

        if !has_escape {
            // Find a cycle to report. Any cycle within the SCC is an infinite recursion loop
            // because there are no escape paths.
            if let Some(raw_chain) = self.find_cycle(scc, &scc_set, |_, _, expr, target| {
                find_ref_range(expr, target)
            }) {
                let chain = self.format_chain(raw_chain, false);
                self.report_cycle(DiagnosticKind::RecursionNoEscape, scc, chain);
            }
            return;
        }

        // 2. Check for infinite loops (Guarded Recursion Analysis)
        // Even if there is an escape, every recursive cycle must consume input (be guarded).
        // We look for a cycle composed entirely of unguarded references.
        if let Some(raw_chain) = self.find_cycle(scc, &scc_set, |_, _, expr, target| {
            find_unguarded_ref_range(expr, target)
        }) {
            let chain = self.format_chain(raw_chain, true);
            self.report_cycle(DiagnosticKind::DirectRecursion, scc, chain);
        }
    }

    /// Finds a cycle within the given set of nodes (SCC).
    /// `get_edge_location` returns the location of a reference from `expr` to `target`.
    fn find_cycle<'b>(
        &self,
        nodes: &'b [String],
        domain: &IndexSet<&'b str>,
        get_edge_location: impl Fn(&Self, SourceId, &Expr, &str) -> Option<TextRange>,
    ) -> Option<Vec<(SourceId, TextRange, &'b str)>> {
        let mut adj = IndexMap::new();
        for name in nodes {
            if let Some((source_id, body)) = self.symbol_table.get_full(name) {
                let neighbors = domain
                    .iter()
                    .filter_map(|target| {
                        get_edge_location(self, source_id, body, target)
                            .map(|range| (*target, source_id, range))
                    })
                    .collect::<Vec<_>>();
                adj.insert(name.as_str(), neighbors);
            }
        }

        let node_strs: Vec<&str> = nodes.iter().map(String::as_str).collect();
        CycleFinder::find(&node_strs, &adj)
    }

    fn format_chain(
        &self,
        raw_chain: Vec<(SourceId, TextRange, &str)>,
        is_unguarded: bool,
    ) -> Vec<(SourceId, TextRange, String)> {
        if raw_chain.len() == 1 {
            let (source_id, range, target) = &raw_chain[0];
            let msg = if is_unguarded {
                "references itself".to_string()
            } else {
                format!("{} references itself", target)
            };
            return vec![(*source_id, *range, msg)];
        }

        let len = raw_chain.len();
        raw_chain
            .into_iter()
            .enumerate()
            .map(|(i, (source_id, range, target))| {
                let msg = if i == len - 1 {
                    format!("references {} (completing cycle)", target)
                } else {
                    format!("references {}", target)
                };
                (source_id, range, msg)
            })
            .collect()
    }

    fn report_cycle(
        &mut self,
        kind: DiagnosticKind,
        scc: &[String],
        chain: Vec<(SourceId, TextRange, String)>,
    ) {
        let (primary_source, primary_loc) = chain
            .first()
            .map(|(s, r, _)| (*s, *r))
            .unwrap_or_else(|| (SourceId::default(), TextRange::empty(0.into())));

        let related_def = if scc.len() > 1 {
            self.find_def_info_containing(scc, primary_loc)
        } else {
            None
        };

        let mut builder = self.diag.report(primary_source, kind, primary_loc);

        for (source_id, range, msg) in chain {
            builder = builder.related_to(source_id, range, msg);
        }

        if let Some((source_id, msg, range)) = related_def {
            builder = builder.related_to(source_id, range, msg);
        }

        builder.emit();
    }

    fn find_def_info_containing(
        &self,
        scc: &[String],
        range: TextRange,
    ) -> Option<(SourceId, String, TextRange)> {
        let name = scc.iter().find(|name| {
            self.symbol_table
                .get(name.as_str())
                .is_some_and(|body| body.text_range().contains_range(range))
        })?;
        let (source_id, def) = self.find_def_by_name(name)?;
        let n = def.name()?;
        Some((
            source_id,
            format!("{} is defined here", name),
            n.text_range(),
        ))
    }

    fn find_def_by_name(&self, name: &str) -> Option<(SourceId, Def)> {
        self.ast_map.iter().find_map(|(source_id, ast)| {
            ast.defs()
                .find(|d| d.name().map(|n| n.text() == name).unwrap_or(false))
                .map(|def| (*source_id, def))
        })
    }
}

struct CycleFinder<'a, 'q> {
    adj: &'a IndexMap<&'q str, Vec<(&'q str, SourceId, TextRange)>>,
    visited: IndexSet<&'q str>,
    on_path: IndexMap<&'q str, usize>,
    path: Vec<&'q str>,
    edges: Vec<(SourceId, TextRange)>,
}

impl<'a, 'q> CycleFinder<'a, 'q> {
    fn find(
        nodes: &[&'q str],
        adj: &'a IndexMap<&'q str, Vec<(&'q str, SourceId, TextRange)>>,
    ) -> Option<Vec<(SourceId, TextRange, &'q str)>> {
        let mut finder = Self {
            adj,
            visited: IndexSet::new(),
            on_path: IndexMap::new(),
            path: Vec::new(),
            edges: Vec::new(),
        };

        for start in nodes {
            if let Some(chain) = finder.dfs(start) {
                return Some(chain);
            }
        }
        None
    }

    fn dfs(&mut self, current: &'q str) -> Option<Vec<(SourceId, TextRange, &'q str)>> {
        if self.on_path.contains_key(current) {
            return None;
        }

        if self.visited.contains(current) {
            return None;
        }

        self.visited.insert(current);
        self.on_path.insert(current, self.path.len());
        self.path.push(current);

        if let Some(neighbors) = self.adj.get(current) {
            for (target, source_id, range) in neighbors {
                if let Some(&start_index) = self.on_path.get(target) {
                    // Cycle detected!
                    let mut chain = Vec::new();
                    for i in start_index..self.path.len() - 1 {
                        let (src, rng) = self.edges[i];
                        chain.push((src, rng, self.path[i + 1]));
                    }
                    chain.push((*source_id, *range, *target));
                    return Some(chain);
                }

                self.edges.push((*source_id, *range));
                if let Some(chain) = self.dfs(target) {
                    return Some(chain);
                }
                self.edges.pop();
            }
        }

        self.path.pop();
        self.on_path.swap_remove(current);
        None
    }
}

fn expr_has_escape(expr: &Expr, scc_names: &IndexSet<&str>) -> bool {
    match expr {
        Expr::Ref(r) => {
            let Some(name_token) = r.name() else {
                return true;
            };
            !scc_names.contains(name_token.text())
        }
        Expr::NamedNode(node) => {
            let children: Vec<_> = node.children().collect();
            children.is_empty() || children.iter().all(|c| expr_has_escape(c, scc_names))
        }
        Expr::AltExpr(_) => expr
            .children()
            .iter()
            .any(|c| expr_has_escape(c, scc_names)),
        Expr::SeqExpr(_) => expr
            .children()
            .iter()
            .all(|c| expr_has_escape(c, scc_names)),
        Expr::QuantifiedExpr(q) => {
            if q.is_optional() {
                return true;
            }
            q.inner()
                .map(|inner| expr_has_escape(&inner, scc_names))
                .unwrap_or(true)
        }
        Expr::CapturedExpr(_) | Expr::FieldExpr(_) => expr
            .children()
            .iter()
            .all(|c| expr_has_escape(c, scc_names)),
        Expr::AnonymousNode(_) => true,
    }
}

fn expr_guarantees_consumption(expr: &Expr) -> bool {
    match expr {
        Expr::NamedNode(_) | Expr::AnonymousNode(_) => true,
        Expr::Ref(_) => false,
        Expr::AltExpr(_) => expr.children().iter().all(expr_guarantees_consumption),
        Expr::SeqExpr(_) => expr.children().iter().any(expr_guarantees_consumption),
        Expr::QuantifiedExpr(q) => {
            !q.is_optional()
                && q.inner()
                    .map(|i| expr_guarantees_consumption(&i))
                    .unwrap_or(false)
        }
        Expr::CapturedExpr(_) | Expr::FieldExpr(_) => {
            expr.children().iter().all(expr_guarantees_consumption)
        }
    }
}

/// Whether to search for any reference or only unguarded ones.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RefSearchMode {
    /// Find any reference to the target.
    Any,
    /// Find only unguarded references (not inside a NamedNode/AnonymousNode).
    Unguarded,
}

struct RefFinder<'a> {
    target: &'a str,
    found: Option<TextRange>,
    mode: RefSearchMode,
}

impl Visitor for RefFinder<'_> {
    fn visit_expr(&mut self, expr: &Expr) {
        if self.found.is_some() {
            return;
        }
        walk_expr(self, expr);
    }

    fn visit_named_node(&mut self, node: &NamedNode) {
        if self.mode == RefSearchMode::Unguarded {
            return; // Guarded: stop recursion
        }
        walk_named_node(self, node);
    }

    fn visit_anonymous_node(&mut self, _node: &AnonymousNode) {
        // AnonymousNode has no child expressions, so nothing to walk.
        // In Unguarded mode this also acts as a guard (stops recursion).
    }

    fn visit_ref(&mut self, r: &Ref) {
        if self.found.is_some() {
            return;
        }
        if let Some(name) = r.name()
            && name.text() == self.target
        {
            self.found = Some(name.text_range());
        }
    }

    fn visit_seq_expr(&mut self, seq: &SeqExpr) {
        for child in seq.children() {
            self.visit_expr(&child);
            if self.found.is_some() {
                return;
            }
            if self.mode == RefSearchMode::Unguarded && expr_guarantees_consumption(&child) {
                return;
            }
        }
    }
}

fn find_ref_range(expr: &Expr, target: &str) -> Option<TextRange> {
    let mut visitor = RefFinder {
        target,
        found: None,
        mode: RefSearchMode::Any,
    };
    visitor.visit_expr(expr);
    visitor.found
}

fn find_unguarded_ref_range(expr: &Expr, target: &str) -> Option<TextRange> {
    let mut visitor = RefFinder {
        target,
        found: None,
        mode: RefSearchMode::Unguarded,
    };
    visitor.visit_expr(expr);
    visitor.found
}
