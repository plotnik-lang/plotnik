//! Dependency analysis and recursion validation.
//!
//! This module computes the dependency graph of definitions, identifies
//! Strongly Connected Components (SCCs), and validates that recursive
//! definitions are well-formed (guarded and escapable).
//!
//! The computed SCCs are exposed in reverse topological order (leaves first),
//! which is useful for passes that need to process dependencies before
//! dependents (like type inference).

use indexmap::{IndexMap, IndexSet};

use super::source_map::SourceId;
use rowan::TextRange;

use crate::Diagnostics;
use crate::diagnostics::DiagnosticKind;
use crate::parser::{AnonymousNode, Def, Expr, NamedNode, Ref, Root, SeqExpr};
use crate::query::symbol_table::SymbolTable;
use crate::query::visitor::{Visitor, walk_expr};

/// Result of dependency analysis.
#[derive(Debug, Clone, Default)]
pub struct DependencyAnalysis<'q> {
    /// Strongly connected components in reverse topological order.
    ///
    /// - `sccs[0]` has no dependencies (or depends only on things not in this list).
    /// - `sccs.last()` depends on everything else.
    /// - Definitions within an SCC are mutually recursive.
    /// - Every definition in the symbol table appears exactly once.
    pub sccs: Vec<Vec<&'q str>>,
}

/// Owned variant of `DependencyAnalysis` for storage in pipeline structs.
#[derive(Debug, Clone, Default)]
pub struct DependencyAnalysisOwned {
    #[allow(dead_code)]
    pub sccs: Vec<Vec<String>>,
}

impl DependencyAnalysis<'_> {
    pub fn to_owned(&self) -> DependencyAnalysisOwned {
        DependencyAnalysisOwned {
            sccs: self
                .sccs
                .iter()
                .map(|scc| scc.iter().map(|s| (*s).to_owned()).collect())
                .collect(),
        }
    }
}

/// Analyze dependencies between definitions.
///
/// Returns the SCCs in reverse topological order.
pub fn analyze_dependencies<'q>(symbol_table: &SymbolTable<'q>) -> DependencyAnalysis<'q> {
    let sccs = SccFinder::find(symbol_table);
    DependencyAnalysis { sccs }
}

/// Validate recursion using the pre-computed dependency analysis.
pub fn validate_recursion<'q>(
    analysis: &DependencyAnalysis<'q>,
    ast_map: &IndexMap<SourceId, Root>,
    symbol_table: &SymbolTable<'q>,
    diag: &mut Diagnostics,
) {
    let mut validator = RecursionValidator {
        ast_map,
        symbol_table,
        diag,
    };
    validator.validate(&analysis.sccs);
}

// -----------------------------------------------------------------------------
// Recursion Validator
// -----------------------------------------------------------------------------

struct RecursionValidator<'a, 'q, 'd> {
    ast_map: &'a IndexMap<SourceId, Root>,
    symbol_table: &'a SymbolTable<'q>,
    diag: &'d mut Diagnostics,
}

impl<'a, 'q, 'd> RecursionValidator<'a, 'q, 'd> {
    fn validate(&mut self, sccs: &[Vec<&'q str>]) {
        for scc in sccs {
            self.validate_scc(scc);
        }
    }

    fn validate_scc(&mut self, scc: &[&'q str]) {
        // Filter out trivial non-recursive components.
        // A component is recursive if it has >1 node, or 1 node that references itself.
        if scc.len() == 1 {
            let name = scc[0];
            let is_self_recursive = self
                .symbol_table
                .get(name)
                .map(|(_, body)| collect_refs(body, self.symbol_table).contains(name))
                .unwrap_or(false);

            if !is_self_recursive {
                return;
            }
        }

        let scc_set: IndexSet<&'q str> = scc.iter().copied().collect();

        // 1. Check for infinite tree structure (Escape Analysis)
        // A valid recursive definition must have a non-recursive path.
        // If NO definition in the SCC has an escape path, the whole group is invalid.
        let has_escape = scc.iter().any(|name| {
            self.symbol_table
                .get(*name)
                .map(|(_, body)| expr_has_escape(body, &scc_set))
                .unwrap_or(true)
        });

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
    fn find_cycle(
        &self,
        nodes: &[&'q str],
        domain: &IndexSet<&'q str>,
        get_edge_location: impl Fn(&Self, SourceId, &Expr, &str) -> Option<TextRange>,
    ) -> Option<Vec<(SourceId, TextRange, &'q str)>> {
        let mut adj = IndexMap::new();
        for name in nodes {
            if let Some(&(source_id, ref body)) = self.symbol_table.get(*name) {
                let neighbors = domain
                    .iter()
                    .filter_map(|target| {
                        get_edge_location(self, source_id, body, target)
                            .map(|range| (*target, source_id, range))
                    })
                    .collect::<Vec<_>>();
                adj.insert(*name, neighbors);
            }
        }

        CycleFinder::find(nodes, &adj)
    }

    fn format_chain(
        &self,
        chain: Vec<(SourceId, TextRange, &'q str)>,
        is_unguarded: bool,
    ) -> Vec<(SourceId, TextRange, String)> {
        if chain.len() == 1 {
            let (source_id, range, target) = &chain[0];
            let msg = if is_unguarded {
                "references itself".to_string()
            } else {
                format!("{} references itself", target)
            };
            return vec![(*source_id, *range, msg)];
        }

        let len = chain.len();
        chain
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
        scc: &[&'q str],
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
        scc: &[&'q str],
        range: TextRange,
    ) -> Option<(SourceId, String, TextRange)> {
        scc.iter()
            .find(|name| {
                self.symbol_table
                    .get(*name)
                    .map(|(_, body)| body.text_range().contains_range(range))
                    .unwrap_or(false)
            })
            .and_then(|name| {
                self.find_def_by_name(name).and_then(|(source_id, def)| {
                    def.name().map(|n| {
                        (
                            source_id,
                            format!("{} is defined here", name),
                            n.text_range(),
                        )
                    })
                })
            })
    }

    fn find_def_by_name(&self, name: &str) -> Option<(SourceId, Def)> {
        self.ast_map.iter().find_map(|(source_id, ast)| {
            ast.defs()
                .find(|d| d.name().map(|n| n.text() == name).unwrap_or(false))
                .map(|def| (*source_id, def))
        })
    }
}

// -----------------------------------------------------------------------------
// SCC Finder (Tarjan's Algorithm)
// -----------------------------------------------------------------------------

struct SccFinder<'a, 'q> {
    symbol_table: &'a SymbolTable<'q>,
    index: usize,
    stack: Vec<&'q str>,
    on_stack: IndexSet<&'q str>,
    indices: IndexMap<&'q str, usize>,
    lowlinks: IndexMap<&'q str, usize>,
    sccs: Vec<Vec<&'q str>>,
}

impl<'a, 'q> SccFinder<'a, 'q> {
    fn find(symbol_table: &'a SymbolTable<'q>) -> Vec<Vec<&'q str>> {
        let mut finder = Self {
            symbol_table,
            index: 0,
            stack: Vec::new(),
            on_stack: IndexSet::new(),
            indices: IndexMap::new(),
            lowlinks: IndexMap::new(),
            sccs: Vec::new(),
        };

        for &name in symbol_table.keys() {
            if !finder.indices.contains_key(name) {
                finder.strongconnect(name);
            }
        }

        finder.sccs
    }

    fn strongconnect(&mut self, name: &'q str) {
        self.indices.insert(name, self.index);
        self.lowlinks.insert(name, self.index);
        self.index += 1;
        self.stack.push(name);
        self.on_stack.insert(name);

        if let Some((_, body)) = self.symbol_table.get(name) {
            let refs = collect_refs(body, self.symbol_table);
            for ref_name in refs {
                // We've already resolved to canonical &'q str in collect_refs
                // so we can use it directly.
                if !self.indices.contains_key(ref_name) {
                    self.strongconnect(ref_name);
                    let ref_lowlink = self.lowlinks[ref_name];
                    let my_lowlink = self.lowlinks.get_mut(name).unwrap();
                    *my_lowlink = (*my_lowlink).min(ref_lowlink);
                } else if self.on_stack.contains(ref_name) {
                    let ref_index = self.indices[ref_name];
                    let my_lowlink = self.lowlinks.get_mut(name).unwrap();
                    *my_lowlink = (*my_lowlink).min(ref_index);
                }
            }
        }

        if self.lowlinks[name] == self.indices[name] {
            let mut scc = Vec::new();
            loop {
                let w = self.stack.pop().unwrap();
                self.on_stack.swap_remove(w);
                scc.push(w);
                if w == name {
                    break;
                }
            }
            self.sccs.push(scc);
        }
    }
}

// -----------------------------------------------------------------------------
// Cycle Finder
// -----------------------------------------------------------------------------

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

// -----------------------------------------------------------------------------
// Helper Visitors
// -----------------------------------------------------------------------------

fn expr_has_escape(expr: &Expr, scc: &IndexSet<&str>) -> bool {
    match expr {
        Expr::Ref(r) => {
            let Some(name_token) = r.name() else {
                return true;
            };
            !scc.contains(name_token.text())
        }
        Expr::NamedNode(node) => {
            let children: Vec<_> = node.children().collect();
            children.is_empty() || children.iter().all(|c| expr_has_escape(c, scc))
        }
        Expr::AltExpr(_) => expr.children().iter().any(|c| expr_has_escape(c, scc)),
        Expr::SeqExpr(_) => expr.children().iter().all(|c| expr_has_escape(c, scc)),
        Expr::QuantifiedExpr(q) => {
            if q.is_optional() {
                return true;
            }
            q.inner()
                .map(|inner| expr_has_escape(&inner, scc))
                .unwrap_or(true)
        }
        Expr::CapturedExpr(_) | Expr::FieldExpr(_) => {
            expr.children().iter().all(|c| expr_has_escape(c, scc))
        }
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

struct RefCollector<'a, 'q> {
    symbol_table: &'a SymbolTable<'q>,
    refs: &'a mut IndexSet<&'q str>,
}

impl<'a, 'q> Visitor for RefCollector<'a, 'q> {
    fn visit_ref(&mut self, r: &Ref) {
        if let Some(name) = r.name() {
            // We immediately resolve to canonical &'q str keys to avoid allocations
            if let Some((&k, _)) = self.symbol_table.get_key_value(name.text()) {
                self.refs.insert(k);
            }
        }
    }
}

fn collect_refs<'q>(expr: &Expr, symbol_table: &SymbolTable<'q>) -> IndexSet<&'q str> {
    let mut refs = IndexSet::new();
    let mut visitor = RefCollector {
        symbol_table,
        refs: &mut refs,
    };
    visitor.visit_expr(expr);
    refs
}

struct RefFinder<'a> {
    target: &'a str,
    found: Option<TextRange>,
}

impl Visitor for RefFinder<'_> {
    fn visit_expr(&mut self, expr: &Expr) {
        if self.found.is_some() {
            return;
        }
        walk_expr(self, expr);
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
}

fn find_ref_range(expr: &Expr, target: &str) -> Option<TextRange> {
    let mut visitor = RefFinder {
        target,
        found: None,
    };
    visitor.visit_expr(expr);
    visitor.found
}

struct UnguardedRefFinder<'a> {
    target: &'a str,
    found: Option<TextRange>,
}

impl Visitor for UnguardedRefFinder<'_> {
    fn visit_expr(&mut self, expr: &Expr) {
        if self.found.is_some() {
            return;
        }
        walk_expr(self, expr);
    }

    fn visit_named_node(&mut self, _node: &NamedNode) {
        // Guarded: stop recursion
    }

    fn visit_anonymous_node(&mut self, _node: &AnonymousNode) {
        // Guarded: stop recursion
    }

    fn visit_ref(&mut self, r: &Ref) {
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
            if expr_guarantees_consumption(&child) {
                return;
            }
        }
    }
}

fn find_unguarded_ref_range(expr: &Expr, target: &str) -> Option<TextRange> {
    let mut visitor = UnguardedRefFinder {
        target,
        found: None,
    };
    visitor.visit_expr(expr);
    visitor.found
}
