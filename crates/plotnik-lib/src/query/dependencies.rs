//! Dependency analysis and recursion validation.
//!
//! This module computes the dependency graph of definitions, identifies
//! Strongly Connected Components (SCCs), and validates that recursive
//! definitions are well-formed (guarded and escapable).
//!
//! The computed SCCs are exposed in reverse topological order (leaves first),
//! which is useful for passes that need to process dependencies before
//! dependents (like type inference).

use std::collections::HashMap;

use indexmap::{IndexMap, IndexSet};
use plotnik_core::{Interner, Symbol};

use super::source_map::SourceId;
use rowan::TextRange;

use crate::Diagnostics;
use crate::diagnostics::DiagnosticKind;
use crate::parser::{AnonymousNode, Def, Expr, NamedNode, Ref, Root, SeqExpr};
use crate::query::symbol_table::SymbolTable;
use crate::query::type_check::DefId;
use crate::query::visitor::{Visitor, walk_expr};

/// Result of dependency analysis.
#[derive(Debug, Clone, Default)]
pub struct DependencyAnalysis {
    /// Strongly connected components in reverse topological order.
    ///
    /// - `sccs[0]` has no dependencies (or depends only on things not in this list).
    /// - `sccs.last()` depends on everything else.
    /// - Definitions within an SCC are mutually recursive.
    /// - Every definition in the symbol table appears exactly once.
    pub sccs: Vec<Vec<String>>,

    /// Maps definition name (Symbol) to its DefId.
    name_to_def: HashMap<Symbol, DefId>,

    /// Maps DefId to definition name Symbol (indexed by DefId).
    def_names: Vec<Symbol>,
}

impl DependencyAnalysis {
    /// Get the DefId for a definition by Symbol.
    pub fn def_id_by_symbol(&self, sym: Symbol) -> Option<DefId> {
        self.name_to_def.get(&sym).copied()
    }

    /// Get the DefId for a definition name (requires interner for lookup).
    pub fn def_id(&self, interner: &Interner, name: &str) -> Option<DefId> {
        // Linear scan - only used during analysis, not hot path
        for (&sym, &def_id) in &self.name_to_def {
            if interner.resolve(sym) == name {
                return Some(def_id);
            }
        }
        None
    }

    /// Get the name Symbol for a DefId.
    pub fn def_name_sym(&self, id: DefId) -> Symbol {
        self.def_names[id.index()]
    }

    /// Get the name string for a DefId.
    pub fn def_name<'a>(&self, interner: &'a Interner, id: DefId) -> &'a str {
        interner.resolve(self.def_names[id.index()])
    }

    /// Number of definitions.
    pub fn def_count(&self) -> usize {
        self.def_names.len()
    }

    /// Get the def_names slice (for seeding TypeContext).
    pub fn def_names(&self) -> &[Symbol] {
        &self.def_names
    }

    /// Get the name_to_def map (for seeding TypeContext).
    pub fn name_to_def(&self) -> &HashMap<Symbol, DefId> {
        &self.name_to_def
    }
}

/// Analyze dependencies between definitions.
///
/// Returns the SCCs in reverse topological order, with DefId mappings.
/// The interner is used to intern definition names as Symbols.
pub fn analyze_dependencies(
    symbol_table: &SymbolTable,
    interner: &mut Interner,
) -> DependencyAnalysis {
    let sccs = SccFinder::find(symbol_table);

    // Assign DefIds in SCC order (leaves first, so dependencies get lower IDs)
    let mut name_to_def = HashMap::new();
    let mut def_names = Vec::new();

    for scc in &sccs {
        for name in scc {
            let sym = interner.intern(name);
            let def_id = DefId::from_raw(def_names.len() as u32);
            name_to_def.insert(sym, def_id);
            def_names.push(sym);
        }
    }

    DependencyAnalysis {
        sccs,
        name_to_def,
        def_names,
    }
}

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
            let Some(body) = self.symbol_table.get(name) else {
                return;
            };
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
    fn find_cycle<'s>(
        &self,
        nodes: &'s [String],
        domain: &IndexSet<&'s str>,
        get_edge_location: impl Fn(&Self, SourceId, &Expr, &str) -> Option<TextRange>,
    ) -> Option<Vec<(SourceId, TextRange, &'s str)>> {
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

struct SccFinder<'a> {
    symbol_table: &'a SymbolTable,
    index: usize,
    stack: Vec<&'a str>,
    on_stack: IndexSet<&'a str>,
    indices: IndexMap<&'a str, usize>,
    lowlinks: IndexMap<&'a str, usize>,
    sccs: Vec<Vec<&'a str>>,
}

impl<'a> SccFinder<'a> {
    fn find(symbol_table: &'a SymbolTable) -> Vec<Vec<String>> {
        let mut finder = Self {
            symbol_table,
            index: 0,
            stack: Vec::new(),
            on_stack: IndexSet::new(),
            indices: IndexMap::new(),
            lowlinks: IndexMap::new(),
            sccs: Vec::new(),
        };

        for name in symbol_table.keys() {
            if !finder.indices.contains_key(name as &str) {
                finder.strongconnect(name);
            }
        }

        finder
            .sccs
            .into_iter()
            .map(|scc| scc.into_iter().map(String::from).collect())
            .collect()
    }

    fn strongconnect(&mut self, name: &'a str) {
        self.indices.insert(name, self.index);
        self.lowlinks.insert(name, self.index);
        self.index += 1;
        self.stack.push(name);
        self.on_stack.insert(name);

        if let Some(body) = self.symbol_table.get(name) {
            let refs = collect_refs(body, self.symbol_table);
            for ref_name in refs {
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
                self.on_stack.swap_remove(&w);
                let done = w == name;
                scc.push(w);
                if done {
                    break;
                }
            }
            self.sccs.push(scc);
        }
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

fn collect_refs<'a>(expr: &Expr, symbol_table: &'a SymbolTable) -> IndexSet<&'a str> {
    let mut refs = IndexSet::new();
    for descendant in expr.as_cst().descendants() {
        let Some(r) = Ref::cast(descendant) else {
            continue;
        };
        let Some(name_tok) = r.name() else { continue };
        let Some(key) = symbol_table.keys().find(|&k| k == name_tok.text()) else {
            continue;
        };
        refs.insert(key);
    }
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
