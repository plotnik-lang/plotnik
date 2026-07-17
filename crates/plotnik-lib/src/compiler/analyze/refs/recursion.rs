//! Recursion validation for definitions.
//!
//! Validates that recursive definitions are well-formed:
//! - Escapable: at least one non-recursive path exists
//! - Progressing: every recursive cycle matches a node before recursing

use indexmap::{IndexMap, IndexSet};
use rowan::TextRange;

use super::dependencies::{DependencyAnalysis, collect_defined_refs};
use crate::compiler::analyze::Located;
use crate::compiler::analyze::names::SymbolTable;
use crate::compiler::analyze::visitor::{Visitor, walk_named_node_pattern, walk_pattern};
use crate::compiler::diagnostics::report::Diagnostics;
use crate::compiler::diagnostics::report::{DiagnosticKind, Span};
use crate::compiler::diagnostics::source::SourceId;
use crate::compiler::parse::ast::{DefRef, NamedNodePattern, Pattern, SeqPattern};
use crate::core::Interner;

pub fn validate_recursion(
    analysis: &DependencyAnalysis,
    symbol_table: &SymbolTable,
    interner: &Interner,
    diag: &mut Diagnostics,
) {
    let mut validator = RecursionValidator {
        symbol_table,
        interner,
        diag,
    };
    validator.validate(analysis);
}

struct RecursionValidator<'a, 'd> {
    symbol_table: &'a SymbolTable,
    interner: &'a Interner,
    diag: &'d mut Diagnostics,
}

#[derive(Clone, Copy)]
enum RecursionFlaw {
    NoEscape,
    NoProgress,
}

impl RecursionFlaw {
    fn search_scope(self) -> CycleSearchScope {
        match self {
            Self::NoEscape => CycleSearchScope::All,
            Self::NoProgress => CycleSearchScope::BeforeProgress,
        }
    }

    fn diagnostic_kind(self) -> DiagnosticKind {
        match self {
            Self::NoEscape => DiagnosticKind::RecursionWithoutEscape,
            Self::NoProgress => DiagnosticKind::RecursionWithoutProgress,
        }
    }

    fn self_reference_message(self, target: &str) -> String {
        match self {
            Self::NoEscape => format!("{target} references itself"),
            Self::NoProgress => "references itself".to_string(),
        }
    }
}

impl<'a, 'd> RecursionValidator<'a, 'd> {
    fn validate(&mut self, analysis: &DependencyAnalysis) {
        for scc in analysis.sccs() {
            let names: Vec<_> = scc
                .iter()
                .map(|def_id| self.interner.resolve(analysis.def_name_sym(*def_id)))
                .collect();
            self.validate_scc(&names);
        }
    }

    fn validate_scc(&mut self, scc: &[&str]) {
        if scc.len() == 1 {
            let name = scc[0];
            let body = self
                .symbol_table
                .body(name)
                .expect("node in SCC must exist in symbol table");
            if !collect_defined_refs(body, self.symbol_table).contains(name) {
                return;
            }
        }

        let scc_set: IndexSet<&str> = scc.iter().copied().collect();

        let has_escape = scc
            .iter()
            .filter_map(|name| self.symbol_table.body(name))
            .any(|body| pattern_has_escape(body, &scc_set));

        if !has_escape {
            // Every cycle is an infinite loop — no escape path exists anywhere in the SCC.
            let kind = RecursionFlaw::NoEscape;
            if let Some(cycle) = self.find_cycle(scc, &scc_set, kind) {
                self.report_cycle(kind, cycle);
            }
            return;
        }

        let kind = RecursionFlaw::NoProgress;
        if let Some(cycle) = self.find_cycle(scc, &scc_set, kind) {
            self.report_cycle(kind, cycle);
        }
    }

    /// Finds a cycle within the given set of nodes (SCC).
    fn find_cycle<'b>(
        &self,
        nodes: &'b [&'b str],
        domain: &IndexSet<&'b str>,
        kind: RecursionFlaw,
    ) -> Option<Vec<CycleEdge<'b>>> {
        let search_scope = kind.search_scope();
        let mut adj = IndexMap::new();
        for &name in nodes {
            if let Some((source_id, body)) = self.symbol_table.definition(name) {
                let neighbors = domain
                    .iter()
                    .filter_map(|target| {
                        search_scope
                            .find_ref_range(source_id, body, target)
                            .map(|range| (*target, Span::new(source_id, range)))
                    })
                    .collect::<Vec<_>>();
                adj.insert(name, neighbors);
            }
        }

        CycleFinder::find(nodes, &adj)
    }

    fn format_chain(&self, cycle: &[CycleEdge<'_>], kind: RecursionFlaw) -> Vec<(Span, String)> {
        if cycle.len() == 1 {
            let edge = cycle[0];
            let msg = kind.self_reference_message(edge.target);
            return vec![(edge.span, msg)];
        }

        cycle
            .iter()
            .enumerate()
            .map(|(index, edge)| {
                let msg = if index == cycle.len() - 1 {
                    format!("references {} (completing cycle)", edge.target)
                } else {
                    format!("references {}", edge.target)
                };
                (edge.span, msg)
            })
            .collect()
    }

    fn report_cycle(&mut self, flaw: RecursionFlaw, cycle: Vec<CycleEdge<'_>>) {
        let primary = cycle
            .first()
            .map(|edge| edge.span)
            .expect("a detected cycle yields a non-empty chain");
        let return_target = cycle
            .last()
            .map(|edge| edge.target)
            .expect("a detected cycle yields a return target");
        let cycle_names = cycle
            .iter()
            .map(|edge| edge.target)
            .collect::<IndexSet<_>>();
        let chain = self.format_chain(&cycle, flaw);

        let related_def = if cycle_names.len() > 1 {
            self.find_def_info_containing(&cycle_names, primary)
        } else {
            None
        };

        let mut builder = self.diag.report(flaw.diagnostic_kind(), primary);

        for (span, msg) in chain {
            builder = builder.related_to(span, msg);
        }

        if let Some((span, msg)) = related_def {
            builder = builder.related_to(span, msg);
        }

        builder = match flaw {
            RecursionFlaw::NoEscape if cycle_names.len() == 1 => builder.hint(format!(
                "add an alternative to `{}` that does not reference `{}`",
                return_target, return_target
            )),
            RecursionFlaw::NoEscape => builder.hint(format!(
                "add an alternative to {} that does not reference any definition in this cycle",
                format_definition_choices(&cycle_names)
            )),
            RecursionFlaw::NoProgress => builder.hint(format!(
                "make every path through this cycle match a syntax-tree node before it returns to `{}`",
                return_target
            )),
        };

        builder.emit();
    }

    fn find_def_info_containing(
        &self,
        cycle_names: &IndexSet<&str>,
        primary: Span,
    ) -> Option<(Span, String)> {
        // A range is only meaningfully contained by a body in the SAME source: two
        // files' bodies can share numeric offsets, so a source-blind containment
        // test would attribute the cycle to whichever file happens to be checked
        // first. Match the source before comparing offsets.
        let name = cycle_names.iter().copied().find(|name| {
            self.symbol_table
                .definition(name)
                .is_some_and(|(def_source, body)| {
                    def_source == primary.source && body.text_range().contains_range(primary.range)
                })
        })?;
        Some((
            self.symbol_table.definition_span(name)?,
            format!("{} is defined here", name),
        ))
    }
}

#[derive(Clone, Copy)]
struct CycleEdge<'a> {
    span: Span,
    target: &'a str,
}

fn format_definition_choices(names: &IndexSet<&str>) -> String {
    let mut names = names
        .iter()
        .map(|name| format!("`{name}`"))
        .collect::<Vec<_>>();
    let last = names
        .pop()
        .expect("a recursive cycle contains at least one definition");
    if names.is_empty() {
        return last;
    }
    format!("{} or {last}", names.join(", "))
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
    ) -> Option<Vec<CycleEdge<'q>>> {
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

    fn dfs(&mut self, current: &'q str) -> Option<Vec<CycleEdge<'q>>> {
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
                        chain.push(CycleEdge {
                            span: self.edges[i],
                            target: self.path[i + 1],
                        });
                    }
                    chain.push(CycleEdge {
                        span: *span,
                        target,
                    });
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
        Pattern::NamedNodePattern(node) => {
            let children: Vec<_> = node.children().collect();
            children.is_empty() || children.iter().all(|c| pattern_has_escape(c, scc_names))
        }
        Pattern::Alternation(_) => pattern
            .children()
            .any(|c| pattern_has_escape(&c, scc_names)),
        Pattern::SeqPattern(_) => pattern
            .children()
            .all(|c| pattern_has_escape(&c, scc_names)),
        Pattern::QuantifiedPattern(q) => {
            if q.is_optional() {
                return true;
            }
            let inner = q.inner().expect("quantified pattern has inner after parse");
            pattern_has_escape(&inner, scc_names)
        }
        Pattern::CapturedPattern(_) | Pattern::FieldPattern(_) => pattern
            .children()
            .all(|c| pattern_has_escape(&c, scc_names)),
        Pattern::AnonymousNodePattern(_) | Pattern::NodeWildcard(_) => true,
    }
}

fn pattern_guarantees_progress(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::NamedNodePattern(_)
        | Pattern::AnonymousNodePattern(_)
        | Pattern::NodeWildcard(_) => true,
        Pattern::DefRef(_) => false,
        Pattern::Alternation(_) => pattern
            .children()
            .all(|child| pattern_guarantees_progress(&child)),
        Pattern::SeqPattern(_) => pattern
            .children()
            .any(|child| pattern_guarantees_progress(&child)),
        Pattern::QuantifiedPattern(q) => {
            !q.is_optional()
                && pattern_guarantees_progress(
                    &q.inner().expect("quantified pattern has inner after parse"),
                )
        }
        Pattern::CapturedPattern(_) | Pattern::FieldPattern(_) => pattern
            .children()
            .all(|child| pattern_guarantees_progress(&child)),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CycleSearchScope {
    All,
    /// No preceding pattern on this path is guaranteed to match a node.
    BeforeProgress,
}

impl CycleSearchScope {
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
    mode: CycleSearchScope,
}

impl Visitor for RefFinder<'_> {
    fn visit_pattern(&mut self, pattern: &Located<Pattern>) {
        if self.found.is_some() {
            return;
        }
        walk_pattern(self, pattern);
    }

    fn visit_named_node_pattern(&mut self, node: &Located<NamedNodePattern>) {
        if self.mode == CycleSearchScope::BeforeProgress {
            return; // Matching this node establishes progress before any nested reference.
        }
        walk_named_node_pattern(self, node);
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
            if self.mode == CycleSearchScope::BeforeProgress
                && pattern_guarantees_progress(child.node())
            {
                return;
            }
        }
    }
}
