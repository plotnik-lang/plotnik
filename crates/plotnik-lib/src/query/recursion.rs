//! Escape path analysis for recursive definitions.
//!
//! Detects patterns that can never match because they require
//! infinitely nested structures (recursion with no escape path),
//! or infinite runtime loops where the cursor never advances (left recursion).

use indexmap::{IndexMap, IndexSet};
use rowan::TextRange;

use super::Query;
use super::visitor::{Visitor, walk_expr};
use crate::diagnostics::DiagnosticKind;
use crate::parser::{AnonymousNode, Def, Expr, NamedNode, Ref, SeqExpr};

impl Query<'_> {
    pub(super) fn validate_recursion(&mut self) {
        let sccs = SccFinder::find(self);

        for scc in sccs {
            self.validate_scc(scc);
        }
    }

    fn validate_scc(&mut self, scc: Vec<String>) {
        let scc_set: IndexSet<&str> = scc.iter().map(|s| s.as_str()).collect();

        // 1. Check for infinite tree structure (Escape Analysis)
        // A valid recursive definition must have a non-recursive path.
        // If NO definition in the SCC has an escape path, the whole group is invalid.
        let has_escape = scc.iter().any(|name| {
            self.symbol_table
                .get(name.as_str())
                .map(|body| expr_has_escape(body, &scc_set))
                .unwrap_or(true)
        });

        if !has_escape {
            // Find a cycle to report. Any cycle within the SCC is an infinite recursion loop
            // because there are no escape paths.
            if let Some(raw_chain) = self.find_cycle(&scc, &scc_set, |_, expr, target| {
                find_ref_range(expr, target)
            }) {
                let chain = self.format_chain(raw_chain, false);
                self.report_cycle(DiagnosticKind::RecursionNoEscape, &scc, chain);
            }
            return;
        }

        // 2. Check for infinite loops (Guarded Recursion Analysis)
        // Even if there is an escape, every recursive cycle must consume input (be guarded).
        // We look for a cycle composed entirely of unguarded references.
        if let Some(raw_chain) = self.find_cycle(&scc, &scc_set, |_, expr, target| {
            find_unguarded_ref_range(expr, target)
        }) {
            let chain = self.format_chain(raw_chain, true);
            self.report_cycle(DiagnosticKind::DirectRecursion, &scc, chain);
        }
    }

    /// Finds a cycle within the given set of nodes (SCC).
    /// `get_edge_location` returns the location of a reference from `expr` to `target`.
    fn find_cycle(
        &self,
        nodes: &[String],
        domain: &IndexSet<&str>,
        get_edge_location: impl Fn(&Query, &Expr, &str) -> Option<TextRange>,
    ) -> Option<Vec<(TextRange, String)>> {
        let mut adj = IndexMap::new();
        for name in nodes {
            if let Some(body) = self.symbol_table.get(name.as_str()) {
                let neighbors = domain
                    .iter()
                    .filter_map(|target| {
                        get_edge_location(self, body, target)
                            .map(|range| (target.to_string(), range))
                    })
                    .collect::<Vec<_>>();
                adj.insert(name.clone(), neighbors);
            }
        }

        CycleFinder::find(nodes, &adj)
    }

    fn format_chain(
        &self,
        chain: Vec<(TextRange, String)>,
        is_unguarded: bool,
    ) -> Vec<(TextRange, String)> {
        if chain.len() == 1 {
            let (range, target) = &chain[0];
            let msg = if is_unguarded {
                "references itself".to_string()
            } else {
                format!("{} references itself", target)
            };
            return vec![(*range, msg)];
        }

        let len = chain.len();
        chain
            .into_iter()
            .enumerate()
            .map(|(i, (range, target))| {
                let msg = if i == len - 1 {
                    format!("references {} (completing cycle)", target)
                } else {
                    format!("references {}", target)
                };
                (range, msg)
            })
            .collect()
    }

    fn report_cycle(
        &mut self,
        kind: DiagnosticKind,
        scc: &[String],
        chain: Vec<(TextRange, String)>,
    ) {
        let primary_loc = chain
            .first()
            .map(|(r, _)| *r)
            .unwrap_or_else(|| TextRange::empty(0.into()));

        let related_def = if scc.len() > 1 {
            self.find_def_info_containing(scc, primary_loc)
        } else {
            None
        };

        let mut builder = self.recursion_diagnostics.report(kind, primary_loc);

        for (range, msg) in chain {
            builder = builder.related_to(msg, range);
        }

        if let Some((msg, range)) = related_def {
            builder = builder.related_to(msg, range);
        }

        builder.emit();
    }

    fn find_def_info_containing(
        &self,
        scc: &[String],
        range: TextRange,
    ) -> Option<(String, TextRange)> {
        scc.iter()
            .find(|name| {
                self.symbol_table
                    .get(name.as_str())
                    .map(|body| body.text_range().contains_range(range))
                    .unwrap_or(false)
            })
            .and_then(|name| {
                self.find_def_by_name(name).and_then(|def| {
                    def.name()
                        .map(|n| (format!("{} is defined here", name), n.text_range()))
                })
            })
    }

    fn find_def_by_name(&self, name: &str) -> Option<Def> {
        self.ast
            .defs()
            .find(|d| d.name().map(|n| n.text() == name).unwrap_or(false))
    }
}

struct CycleFinder<'a> {
    adj: &'a IndexMap<String, Vec<(String, TextRange)>>,
    visited: IndexSet<String>,
    on_path: IndexMap<String, usize>,
    path: Vec<String>,
    edges: Vec<TextRange>,
}

impl<'a> CycleFinder<'a> {
    fn find(
        nodes: &[String],
        adj: &'a IndexMap<String, Vec<(String, TextRange)>>,
    ) -> Option<Vec<(TextRange, String)>> {
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

    fn dfs(&mut self, current: &String) -> Option<Vec<(TextRange, String)>> {
        if self.on_path.contains_key(current) {
            return None;
        }

        if self.visited.contains(current) {
            return None;
        }

        self.visited.insert(current.clone());
        self.on_path.insert(current.clone(), self.path.len());
        self.path.push(current.clone());

        if let Some(neighbors) = self.adj.get(current) {
            for (target, range) in neighbors {
                if let Some(&start_index) = self.on_path.get(target) {
                    // Cycle detected!
                    let mut chain = Vec::new();
                    for i in start_index..self.path.len() - 1 {
                        chain.push((self.edges[i], self.path[i + 1].clone()));
                    }
                    chain.push((*range, target.clone()));
                    return Some(chain);
                }

                self.edges.push(*range);
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

struct SccFinder<'a, 'src> {
    query: &'a Query<'src>,
    index: usize,
    stack: Vec<String>,
    on_stack: IndexSet<String>,
    indices: IndexMap<String, usize>,
    lowlinks: IndexMap<String, usize>,
    sccs: Vec<Vec<String>>,
}

impl<'a, 'src> SccFinder<'a, 'src> {
    fn find(query: &'a Query<'src>) -> Vec<Vec<String>> {
        let mut finder = Self {
            query,
            index: 0,
            stack: Vec::new(),
            on_stack: IndexSet::new(),
            indices: IndexMap::new(),
            lowlinks: IndexMap::new(),
            sccs: Vec::new(),
        };

        for name in query.symbol_table.keys() {
            if !finder.indices.contains_key(*name) {
                finder.strongconnect(name);
            }
        }

        finder
            .sccs
            .into_iter()
            .filter(|scc| {
                scc.len() > 1
                    || query
                        .symbol_table
                        .get(scc[0].as_str())
                        .map(|body| collect_refs(body).contains(scc[0].as_str()))
                        .unwrap_or(false)
            })
            .collect()
    }

    fn strongconnect(&mut self, name: &str) {
        self.indices.insert(name.to_string(), self.index);
        self.lowlinks.insert(name.to_string(), self.index);
        self.index += 1;
        self.stack.push(name.to_string());
        self.on_stack.insert(name.to_string());

        if let Some(body) = self.query.symbol_table.get(name) {
            let refs = collect_refs(body);
            for ref_name in refs {
                if !self.query.symbol_table.contains_key(ref_name.as_str()) {
                    continue;
                }

                if !self.indices.contains_key(&ref_name) {
                    self.strongconnect(&ref_name);
                    let ref_lowlink = self.lowlinks[&ref_name];
                    let my_lowlink = self.lowlinks.get_mut(name).unwrap();
                    *my_lowlink = (*my_lowlink).min(ref_lowlink);
                } else if self.on_stack.contains(&ref_name) {
                    let ref_index = self.indices[&ref_name];
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
                scc.push(w.clone());
                if w == name {
                    break;
                }
            }
            self.sccs.push(scc);
        }
    }
}

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

struct RefCollector<'a> {
    refs: &'a mut IndexSet<String>,
}

impl Visitor for RefCollector<'_> {
    fn visit_ref(&mut self, r: &Ref) {
        if let Some(name) = r.name() {
            self.refs.insert(name.text().to_string());
        }
    }
}

fn collect_refs(expr: &Expr) -> IndexSet<String> {
    let mut refs = IndexSet::new();
    let mut visitor = RefCollector { refs: &mut refs };
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
