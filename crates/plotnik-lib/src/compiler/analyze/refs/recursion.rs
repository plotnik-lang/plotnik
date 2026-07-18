//! Recursion validation for definitions.
//!
//! Validates that recursive definitions are well-formed:
//! - Escapable: at least one non-recursive path exists
//! - Progressing: every recursive cycle matches a node before recursing

use indexmap::{IndexMap, IndexSet};
use rowan::TextRange;

use super::DefinitionGraph;
use crate::compiler::analyze::Located;
use crate::compiler::analyze::visitor::{Visitor, walk_named_node_pattern, walk_pattern};
use crate::compiler::diagnostics::report::Diagnostics;
use crate::compiler::diagnostics::report::{DiagnosticKind, Span};
use crate::compiler::diagnostics::source::SourceId;
use crate::compiler::ids::DefId;
use crate::compiler::parse::ast::{DefRef, NamedNodePattern, Pattern, SeqPattern};
use crate::core::Interner;

pub(in crate::compiler) fn validate_recursion(
    definitions: &DefinitionGraph,
    interner: &Interner,
    diag: &mut Diagnostics,
) {
    let mut validator = RecursionValidator {
        definitions,
        interner,
        diag,
    };
    for scc in definitions.sccs() {
        validator.validate_scc(scc);
    }
}

struct RecursionValidator<'a, 'd> {
    definitions: &'a DefinitionGraph,
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
    fn validate_scc(&mut self, scc: &[DefId]) {
        if scc.len() == 1 && !self.definitions.is_recursive(scc[0]) {
            return;
        }

        let scc_set: IndexSet<DefId> = scc.iter().copied().collect();

        let has_escape = scc
            .iter()
            .map(|&def_id| self.definitions.definition(def_id).body())
            .any(|body| pattern_has_escape(body, &scc_set, self.definitions, self.interner));

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
    fn find_cycle(
        &self,
        nodes: &[DefId],
        domain: &IndexSet<DefId>,
        kind: RecursionFlaw,
    ) -> Option<Vec<CycleEdge>> {
        let search_scope = kind.search_scope();
        let mut adj = IndexMap::new();
        for &def_id in nodes {
            let definition = self.definitions.definition(def_id);
            let source = definition.source();
            let neighbors = domain
                .iter()
                .filter_map(|&target| {
                    let target_name = self
                        .interner
                        .resolve(self.definitions.definition(target).name());
                    search_scope
                        .find_ref_range(source, definition.body(), target_name)
                        .map(|range| (target, Span::new(source, range)))
                })
                .collect::<Vec<_>>();
            adj.insert(def_id, neighbors);
        }

        CycleFinder::find(nodes, &adj)
    }

    fn format_chain(&self, cycle: &[CycleEdge], kind: RecursionFlaw) -> Vec<(Span, String)> {
        if cycle.len() == 1 {
            let edge = cycle[0];
            let msg = kind.self_reference_message(self.name(edge.target));
            return vec![(edge.span, msg)];
        }

        cycle
            .iter()
            .enumerate()
            .map(|(index, edge)| {
                let target = self.name(edge.target);
                let msg = if index == cycle.len() - 1 {
                    format!("references {target} (completing cycle)")
                } else {
                    format!("references {target}")
                };
                (edge.span, msg)
            })
            .collect()
    }

    fn report_cycle(&mut self, flaw: RecursionFlaw, cycle: Vec<CycleEdge>) {
        let primary = cycle
            .first()
            .map(|edge| edge.span)
            .expect("a detected cycle yields a non-empty chain");
        let return_target = cycle
            .last()
            .map(|edge| edge.target)
            .expect("a detected cycle yields a return target");
        let cycle_definitions = cycle
            .iter()
            .map(|edge| edge.target)
            .collect::<IndexSet<_>>();
        let chain = self.format_chain(&cycle, flaw);

        let related_definition = if cycle_definitions.len() > 1 {
            self.find_containing_definition(&cycle_definitions, primary)
        } else {
            None
        };
        let hint = match flaw {
            RecursionFlaw::NoEscape if cycle_definitions.len() == 1 => format!(
                "add an alternative to `{}` that does not reference `{}`",
                self.name(return_target),
                self.name(return_target),
            ),
            RecursionFlaw::NoEscape => format!(
                "add an alternative to {} that does not reference any definition in this cycle",
                self.format_definition_choices(&cycle_definitions)
            ),
            RecursionFlaw::NoProgress => format!(
                "make every path through this cycle match a syntax-tree node before it returns to `{}`",
                self.name(return_target),
            ),
        };

        let mut builder = self.diag.report(flaw.diagnostic_kind(), primary);

        for (span, msg) in chain {
            builder = builder.related_to(span, msg);
        }

        if let Some((span, msg)) = related_definition {
            builder = builder.related_to(span, msg);
        }

        builder.hint(hint).emit();
    }

    fn find_containing_definition(
        &self,
        cycle_definitions: &IndexSet<DefId>,
        primary: Span,
    ) -> Option<(Span, String)> {
        // A range is only meaningfully contained by a body in the SAME source: two
        // files' bodies can share numeric offsets, so a source-blind containment
        // test would attribute the cycle to whichever file happens to be checked
        // first. Match the source before comparing offsets.
        let def_id = cycle_definitions.iter().copied().find(|&def_id| {
            let definition = self.definitions.definition(def_id);
            definition.source() == primary.source
                && definition.body().text_range().contains_range(primary.range)
        })?;
        let definition = self.definitions.definition(def_id);
        Some((
            definition.span(),
            format!("{} is defined here", self.name(def_id)),
        ))
    }

    fn name(&self, def_id: DefId) -> &str {
        self.interner
            .resolve(self.definitions.definition(def_id).name())
    }

    fn format_definition_choices(&self, definitions: &IndexSet<DefId>) -> String {
        let mut names = definitions
            .iter()
            .map(|&def_id| format!("`{}`", self.name(def_id)))
            .collect::<Vec<_>>();
        let last = names
            .pop()
            .expect("a recursive cycle contains at least one definition");
        if names.is_empty() {
            return last;
        }
        format!("{} or {last}", names.join(", "))
    }
}

#[derive(Clone, Copy)]
struct CycleEdge {
    span: Span,
    target: DefId,
}

struct CycleFinder<'a> {
    adj: &'a IndexMap<DefId, Vec<(DefId, Span)>>,
    visited: IndexSet<DefId>,
    on_path: IndexMap<DefId, usize>,
    path: Vec<DefId>,
    edges: Vec<Span>,
}

impl<'a> CycleFinder<'a> {
    fn find(
        nodes: &[DefId],
        adj: &'a IndexMap<DefId, Vec<(DefId, Span)>>,
    ) -> Option<Vec<CycleEdge>> {
        let mut finder = Self {
            adj,
            visited: IndexSet::new(),
            on_path: IndexMap::new(),
            path: Vec::new(),
            edges: Vec::new(),
        };

        for &start in nodes {
            if let Some(chain) = finder.dfs(start) {
                return Some(chain);
            }
        }
        None
    }

    fn dfs(&mut self, current: DefId) -> Option<Vec<CycleEdge>> {
        if self.visited.contains(&current) {
            return None;
        }

        self.visited.insert(current);
        self.on_path.insert(current, self.path.len());
        self.path.push(current);

        if let Some(neighbors) = self.adj.get(&current) {
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
                        target: *target,
                    });
                    return Some(chain);
                }

                self.edges.push(*span);
                if let Some(chain) = self.dfs(*target) {
                    return Some(chain);
                }
                self.edges.pop();
            }
        }

        self.path.pop();
        self.on_path.swap_remove(&current);
        None
    }
}

fn pattern_has_escape(
    pattern: &Pattern,
    scc: &IndexSet<DefId>,
    definitions: &DefinitionGraph,
    interner: &Interner,
) -> bool {
    match pattern {
        Pattern::DefRef(r) => {
            let Some(name_token) = r.name() else {
                return true;
            };
            let Some(def_id) = definitions.id_for_name(interner, name_token.text()) else {
                return true;
            };
            !scc.contains(&def_id)
        }
        Pattern::NamedNodePattern(node) => {
            let children: Vec<_> = node.children().collect();
            children.is_empty()
                || children
                    .iter()
                    .all(|c| pattern_has_escape(c, scc, definitions, interner))
        }
        Pattern::Alternation(_) => pattern
            .children()
            .any(|c| pattern_has_escape(&c, scc, definitions, interner)),
        Pattern::SeqPattern(_) => pattern
            .children()
            .all(|c| pattern_has_escape(&c, scc, definitions, interner)),
        Pattern::QuantifiedPattern(q) => {
            if q.is_optional() {
                return true;
            }
            let inner = q.inner().expect("quantified pattern has inner after parse");
            pattern_has_escape(&inner, scc, definitions, interner)
        }
        Pattern::CapturedPattern(_) | Pattern::FieldPattern(_) => pattern
            .children()
            .all(|c| pattern_has_escape(&c, scc, definitions, interner)),
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
