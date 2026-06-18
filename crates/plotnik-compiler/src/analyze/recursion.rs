//! Recursion validation for definitions.
//!
//! Validates that recursive definitions are well-formed:
//! - Escapable: at least one non-recursive path exists
//! - Guarded: every recursive cycle consumes input

use indexmap::{IndexMap, IndexSet};
use rowan::TextRange;

use super::dependencies::{DependencyAnalysis, collect_refs};
use super::symbol_table::SymbolTable;
use super::visitor::{Visitor, walk_pattern, walk_node_pattern};
use crate::Diagnostics;
use crate::diagnostics::DiagnosticKind;
use crate::parser::{TokenPattern, Def, Pattern, NodePattern, Ref, Root, SeqPattern};
use crate::query::SourceId;

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
    validator.validate(analysis.sccs());
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
        if scc.len() == 1 {
            let name = &scc[0];
            let body = self
                .symbol_table
                .body(name)
                .expect("node in SCC must exist in symbol table");
            if !collect_refs(body, self.symbol_table).contains(name.as_str()) {
                return;
            }
        }

        let scc_set: IndexSet<&str> = scc.iter().map(String::as_str).collect();

        let has_escape = scc
            .iter()
            .filter_map(|name| self.symbol_table.body(name))
            .any(|body| expr_has_escape(body, &scc_set));

        if !has_escape {
            // Every cycle is an infinite loop — no escape path exists anywhere in the SCC.
            if let Some(raw_chain) = self.find_cycle(scc, &scc_set, |_, _, pattern, target| {
                find_ref_range(pattern, target)
            }) {
                let chain = self.format_chain(raw_chain, false);
                self.report_cycle(DiagnosticKind::RecursionNoEscape, scc, chain);
            }
            return;
        }

        if let Some(raw_chain) = self.find_cycle(scc, &scc_set, |_, _, pattern, target| {
            find_unguarded_ref_range(pattern, target)
        }) {
            let chain = self.format_chain(raw_chain, true);
            self.report_cycle(DiagnosticKind::DirectRecursion, scc, chain);
        }
    }

    /// Finds a cycle within the given set of nodes (SCC).
    /// `get_edge_location` returns the location of a reference from `pattern` to `target`.
    fn find_cycle<'b>(
        &self,
        nodes: &'b [String],
        domain: &IndexSet<&'b str>,
        get_edge_location: impl Fn(&Self, SourceId, &Pattern, &str) -> Option<TextRange>,
    ) -> Option<Vec<(SourceId, TextRange, &'b str)>> {
        let mut adj = IndexMap::new();
        for name in nodes {
            if let Some((source_id, body)) = self.symbol_table.definition(name) {
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
                .body(name.as_str())
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

fn expr_has_escape(pattern: &Pattern, scc_names: &IndexSet<&str>) -> bool {
    match pattern {
        Pattern::Ref(r) => {
            let Some(name_token) = r.name() else {
                return true;
            };
            !scc_names.contains(name_token.text())
        }
        Pattern::NodePattern(node) => {
            let children: Vec<_> = node.children().collect();
            children.is_empty() || children.iter().all(|c| expr_has_escape(c, scc_names))
        }
        Pattern::Union(_) | Pattern::Enum(_) => pattern
            .children()
            .iter()
            .any(|c| expr_has_escape(c, scc_names)),
        Pattern::SeqPattern(_) => pattern
            .children()
            .iter()
            .all(|c| expr_has_escape(c, scc_names)),
        Pattern::QuantifiedPattern(q) => {
            if q.is_optional() {
                return true;
            }
            q.inner()
                .map(|inner| expr_has_escape(&inner, scc_names))
                .unwrap_or(true)
        }
        Pattern::CapturedPattern(_) | Pattern::FieldPattern(_) => pattern
            .children()
            .iter()
            .all(|c| expr_has_escape(c, scc_names)),
        Pattern::TokenPattern(_) => true,
    }
}

fn expr_guarantees_consumption(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::NodePattern(_) | Pattern::TokenPattern(_) => true,
        Pattern::Ref(_) => false,
        Pattern::Union(_) | Pattern::Enum(_) => {
            pattern.children().iter().all(expr_guarantees_consumption)
        }
        Pattern::SeqPattern(_) => pattern.children().iter().any(expr_guarantees_consumption),
        Pattern::QuantifiedPattern(q) => {
            !q.is_optional()
                && q.inner()
                    .map(|i| expr_guarantees_consumption(&i))
                    .unwrap_or(false)
        }
        Pattern::CapturedPattern(_) | Pattern::FieldPattern(_) => {
            pattern.children().iter().all(expr_guarantees_consumption)
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RefSearchMode {
    Any,
    /// Not inside a `NodePattern`/`TokenPattern` (those consume input, so the cycle is guarded).
    Unguarded,
}

struct RefFinder<'a> {
    target: &'a str,
    found: Option<TextRange>,
    mode: RefSearchMode,
}

impl Visitor for RefFinder<'_> {
    fn visit_pattern(&mut self, pattern: &Pattern) {
        if self.found.is_some() {
            return;
        }
        walk_pattern(self, pattern);
    }

    fn visit_node_pattern(&mut self, node: &NodePattern) {
        if self.mode == RefSearchMode::Unguarded {
            return; // Guarded: stop recursion
        }
        walk_node_pattern(self, node);
    }

    fn visit_token_pattern(&mut self, _node: &TokenPattern) {
        // TokenPattern has no child expressions, so nothing to walk.
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

    fn visit_seq_pattern(&mut self, seq: &SeqPattern) {
        for child in seq.children() {
            self.visit_pattern(&child);
            if self.found.is_some() {
                return;
            }
            if self.mode == RefSearchMode::Unguarded && expr_guarantees_consumption(&child) {
                return;
            }
        }
    }
}

fn find_ref_range(pattern: &Pattern, target: &str) -> Option<TextRange> {
    let mut visitor = RefFinder {
        target,
        found: None,
        mode: RefSearchMode::Any,
    };
    visitor.visit_pattern(pattern);
    visitor.found
}

fn find_unguarded_ref_range(pattern: &Pattern, target: &str) -> Option<TextRange> {
    let mut visitor = RefFinder {
        target,
        found: None,
        mode: RefSearchMode::Unguarded,
    };
    visitor.visit_pattern(pattern);
    visitor.found
}
