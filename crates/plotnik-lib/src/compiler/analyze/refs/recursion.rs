//! Recursion validation for definitions.
//!
//! Validates that recursive definitions are well-formed:
//! - Escapable: at least one non-recursive path exists
//! - Guarded: every recursive cycle consumes input

use indexmap::{IndexMap, IndexSet};
use rowan::TextRange;

use super::dependencies::{DependencyAnalysis, collect_refs};
use crate::compiler::analyze::Located;
use crate::compiler::analyze::names::SymbolTable;
use crate::compiler::analyze::visitor::{Visitor, walk_node_pattern, walk_pattern};
use crate::compiler::diagnostics::diagnostics::Diagnostics;
use crate::compiler::diagnostics::diagnostics::{DiagnosticKind, Span};
use crate::compiler::diagnostics::source::SourceId;
use crate::compiler::parse::ast::{Def, NodePattern, Pattern, DefRef, Root, SeqPattern, TokenPattern};

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

#[derive(Clone, Copy)]
enum CycleKind {
    NoEscape,
    Unguarded,
}

impl CycleKind {
    fn search_mode(self) -> RefSearchMode {
        match self {
            Self::NoEscape => RefSearchMode::Any,
            Self::Unguarded => RefSearchMode::Unguarded,
        }
    }

    fn diagnostic_kind(self) -> DiagnosticKind {
        match self {
            Self::NoEscape => DiagnosticKind::RecursionNoEscape,
            Self::Unguarded => DiagnosticKind::DirectRecursion,
        }
    }

    fn self_reference_message(self, target: &str) -> String {
        match self {
            Self::NoEscape => format!("{target} references itself"),
            Self::Unguarded => "references itself".to_string(),
        }
    }
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
            .any(|body| pattern_has_escape(body, &scc_set));

        if !has_escape {
            // Every cycle is an infinite loop — no escape path exists anywhere in the SCC.
            let kind = CycleKind::NoEscape;
            if let Some(raw_chain) = self.find_cycle(scc, &scc_set, kind) {
                let chain = self.format_chain(raw_chain, kind);
                self.report_cycle(kind.diagnostic_kind(), scc, chain);
            }
            return;
        }

        let kind = CycleKind::Unguarded;
        if let Some(raw_chain) = self.find_cycle(scc, &scc_set, kind) {
            let chain = self.format_chain(raw_chain, kind);
            self.report_cycle(kind.diagnostic_kind(), scc, chain);
        }
    }

    /// Finds a cycle within the given set of nodes (SCC).
    fn find_cycle<'b>(
        &self,
        nodes: &'b [String],
        domain: &IndexSet<&'b str>,
        kind: CycleKind,
    ) -> Option<Vec<(Span, &'b str)>> {
        let search_mode = kind.search_mode();
        let mut adj = IndexMap::new();
        for name in nodes {
            if let Some((source_id, body)) = self.symbol_table.definition(name) {
                let neighbors = domain
                    .iter()
                    .filter_map(|target| {
                        search_mode
                            .find_ref_range(source_id, body, target)
                            .map(|range| (*target, Span::new(source_id, range)))
                    })
                    .collect::<Vec<_>>();
                adj.insert(name.as_str(), neighbors);
            }
        }

        let node_strs: Vec<&str> = nodes.iter().map(String::as_str).collect();
        CycleFinder::find(&node_strs, &adj)
    }

    fn format_chain(&self, raw_chain: Vec<(Span, &str)>, kind: CycleKind) -> Vec<(Span, String)> {
        if raw_chain.len() == 1 {
            let (span, target) = &raw_chain[0];
            let msg = kind.self_reference_message(target);
            return vec![(*span, msg)];
        }

        let len = raw_chain.len();
        raw_chain
            .into_iter()
            .enumerate()
            .map(|(i, (span, target))| {
                let msg = if i == len - 1 {
                    format!("references {} (completing cycle)", target)
                } else {
                    format!("references {}", target)
                };
                (span, msg)
            })
            .collect()
    }

    fn report_cycle(&mut self, kind: DiagnosticKind, scc: &[String], chain: Vec<(Span, String)>) {
        let primary = chain
            .first()
            .map(|(span, _)| *span)
            .expect("a detected cycle yields a non-empty chain");

        let related_def = if scc.len() > 1 {
            self.find_def_info_containing(scc, primary)
        } else {
            None
        };

        let mut builder = self.diag.report(kind, primary);

        for (span, msg) in chain {
            builder = builder.related_to(span, msg);
        }

        if let Some((span, msg)) = related_def {
            builder = builder.related_to(span, msg);
        }

        builder.emit();
    }

    fn find_def_info_containing(&self, scc: &[String], primary: Span) -> Option<(Span, String)> {
        // A range is only meaningfully contained by a body in the SAME source: two
        // files' bodies can share numeric offsets, so a source-blind containment
        // test would attribute the cycle to whichever file happens to be checked
        // first. Match the source before comparing offsets.
        let name = scc.iter().find(|name| {
            self.symbol_table
                .definition(name.as_str())
                .is_some_and(|(def_source, body)| {
                    def_source == primary.source && body.text_range().contains_range(primary.range)
                })
        })?;
        let (source_id, def) = self.find_def_by_name(name)?;
        let n = def.name()?;
        Some((
            Span::new(source_id, n.text_range()),
            format!("{} is defined here", name),
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
    adj: &'a IndexMap<&'q str, Vec<(&'q str, Span)>>,
    visited: IndexSet<&'q str>,
    on_path: IndexMap<&'q str, usize>,
    path: Vec<&'q str>,
    edges: Vec<Span>,
}

impl<'a, 'q> CycleFinder<'a, 'q> {
    fn find(
        nodes: &[&'q str],
        adj: &'a IndexMap<&'q str, Vec<(&'q str, Span)>>,
    ) -> Option<Vec<(Span, &'q str)>> {
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

    fn dfs(&mut self, current: &'q str) -> Option<Vec<(Span, &'q str)>> {
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
            for (target, span) in neighbors {
                if let Some(&start_index) = self.on_path.get(target) {
                    let mut chain = Vec::new();
                    for i in start_index..self.path.len() - 1 {
                        chain.push((self.edges[i], self.path[i + 1]));
                    }
                    chain.push((*span, *target));
                    return Some(chain);
                }

                self.edges.push(*span);
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

fn pattern_has_escape(pattern: &Pattern, scc_names: &IndexSet<&str>) -> bool {
    match pattern {
        Pattern::DefRef(r) => {
            let Some(name_token) = r.name() else {
                return true;
            };
            !scc_names.contains(name_token.text())
        }
        Pattern::NodePattern(node) => {
            let children: Vec<_> = node.children().collect();
            children.is_empty() || children.iter().all(|c| pattern_has_escape(c, scc_names))
        }
        Pattern::Union(_) | Pattern::Enum(_) => pattern
            .children()
            .iter()
            .any(|c| pattern_has_escape(c, scc_names)),
        Pattern::SeqPattern(_) => pattern
            .children()
            .iter()
            .all(|c| pattern_has_escape(c, scc_names)),
        Pattern::QuantifiedPattern(q) => {
            if q.is_optional() {
                return true;
            }
            q.inner()
                .map(|inner| pattern_has_escape(&inner, scc_names))
                .unwrap_or(true)
        }
        Pattern::CapturedPattern(_) | Pattern::FieldPattern(_) => pattern
            .children()
            .iter()
            .all(|c| pattern_has_escape(c, scc_names)),
        Pattern::TokenPattern(_) => true,
    }
}

fn pattern_consumes_input(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::NodePattern(_) | Pattern::TokenPattern(_) => true,
        Pattern::DefRef(_) => false,
        Pattern::Union(_) | Pattern::Enum(_) => {
            pattern.children().iter().all(pattern_consumes_input)
        }
        Pattern::SeqPattern(_) => pattern.children().iter().any(pattern_consumes_input),
        Pattern::QuantifiedPattern(q) => {
            !q.is_optional()
                && q.inner()
                    .map(|i| pattern_consumes_input(&i))
                    .unwrap_or(false)
        }
        Pattern::CapturedPattern(_) | Pattern::FieldPattern(_) => {
            pattern.children().iter().all(pattern_consumes_input)
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RefSearchMode {
    Any,
    /// Not inside a `NodePattern`/`TokenPattern` (those consume input, so the cycle is guarded).
    Unguarded,
}

impl RefSearchMode {
    fn find_ref_range(
        self,
        source: SourceId,
        pattern: &Pattern,
        target: &str,
    ) -> Option<TextRange> {
        let mut visitor = RefFinder {
            target,
            found: None,
            mode: self,
        };
        visitor.visit_pattern(&Located::new(source, pattern.clone()));
        visitor.found
    }
}

struct RefFinder<'a> {
    target: &'a str,
    found: Option<TextRange>,
    mode: RefSearchMode,
}

impl Visitor for RefFinder<'_> {
    fn visit_pattern(&mut self, pattern: &Located<Pattern>) {
        if self.found.is_some() {
            return;
        }
        walk_pattern(self, pattern);
    }

    fn visit_node_pattern(&mut self, node: &Located<NodePattern>) {
        if self.mode == RefSearchMode::Unguarded {
            return; // Guarded: stop recursion
        }
        walk_node_pattern(self, node);
    }

    fn visit_token_pattern(&mut self, _node: &Located<TokenPattern>) {
        // TokenPattern has no child patterns, so nothing to walk.
        // In Unguarded mode this also acts as a guard (stops recursion).
    }

    fn visit_def_ref(&mut self, r: &Located<DefRef>) {
        if self.found.is_some() {
            return;
        }
        if let Some(name) = r.node().name()
            && name.text() == self.target
        {
            self.found = Some(name.text_range());
        }
    }

    fn visit_seq_pattern(&mut self, seq: &Located<SeqPattern>) {
        for child in seq.node().children() {
            let child = seq.wrap(child);
            self.visit_pattern(&child);
            if self.found.is_some() {
                return;
            }
            if self.mode == RefSearchMode::Unguarded && pattern_consumes_input(child.node()) {
                return;
            }
        }
    }
}
