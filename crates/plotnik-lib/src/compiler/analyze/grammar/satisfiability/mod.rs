//! Sequence, anchor, and arity satisfiability.
//!
//! The structural check (`../check.rs`) validates each query position in isolation —
//! kind exists, field is on the node, child kind is admissible. It is order-,
//! adjacency-, and arity-blind, so `(function_declaration .! (identifier))` and
//! `(array (statement))` slip through. This pass closes that gap: it threads the
//! grammar's productions through a per-query-node child automaton (`automaton.rs`)
//! under a least fixed point (`engine.rs`), and rejects a query node exactly when no
//! tree the grammar can produce realizes its children. `diagnose.rs` turns a failure
//! into a message that points at the deepest culprit and explains the obstacle.
//!
//! The goal is completeness: reject every query the grammar can never match. What this
//! pass rejects *is* its value, so an impossible query that slips through is a real
//! defect — one to keep closing, not to excuse because the pass played it safe.
//! Correctness is how much it catches, not merely what it refrains from rejecting;
//! "couldn't prove it impossible" is the floor, not the bar. We are not there yet, and
//! the grammar model we keep is lossy, so some impossibilities are still invisible here.
//!
//! One rule is absolute and shapes how we reach for that goal: never reject a query the
//! grammar *can* match. A false rejection blocks legitimate work, so it is the single
//! failure we must prevent — when a verdict is genuinely undecidable, accept. That is
//! why the walk only reports at `Required` positions: a concrete-kind node not under an
//! alternation branch or quantified body, where a failure cannot be excused by a
//! sibling branch or zero repetitions.

mod automaton;
mod diagnose;
mod engine;
mod state_set;

pub use engine::DEFAULT_SATISFIABILITY_STEP_BUDGET;

#[cfg(test)]
mod state_set_tests;

use indexmap::IndexMap;

use crate::compiler::analyze::Located;
use crate::compiler::analyze::names::SymbolTable;
use crate::compiler::diagnostics::report::Diagnostics;
use crate::compiler::diagnostics::source::{SourceId, SourceMap};
use crate::compiler::parse::ast::{NodePattern, Pattern, Root, token_src};
use crate::compiler::parse::cst::SyntaxKind;
use crate::core::NodeKindId;
use crate::core::grammar::Grammar;

use super::participation::Participation;
use automaton::AutomatonContext;
use engine::SatisfiabilitySolver;

/// The threaded dependencies of the satisfiability pass.
pub(super) struct SatisfiabilityInput<'a> {
    pub(super) grammar: &'a Grammar,
    pub(super) symbol_table: &'a SymbolTable,
    pub(super) source_map: &'a SourceMap,
    pub(super) ast_map: &'a IndexMap<SourceId, Root>,
    /// The query's structural-depth ceiling (the parser's `max_depth`), reused to bound
    /// automaton construction so an inlining chain cannot outrun the native stack.
    pub(super) max_depth: u32,
    /// Work ceiling for the satisfiability solve — a wide child list drives a quadratic
    /// fixed point, so past this many state-visits the query is rejected as too complex.
    pub(super) satisfiability_step_budget: u64,
}

/// Run the satisfiability pass over every definition, reporting impossible patterns.
pub(super) fn check(input: SatisfiabilityInput<'_>, diag: &mut Diagnostics) {
    // A metadata-only grammar has no retained productions, so nothing can be decided;
    // accept everything rather than reason from an empty model.
    if input.grammar.structure().variables().is_empty() {
        return;
    }

    let ctx = AutomatonContext {
        grammar: input.grammar,
        symbol_table: input.symbol_table,
        source_map: input.source_map,
    };

    let mut solver =
        SatisfiabilitySolver::checking(ctx, input.max_depth, input.satisfiability_step_budget);
    let mut reporter = Reporter {
        solver: &mut solver,
        diag,
    };

    // Each definition body must be matchable in its own right — the structural check's
    // stance — so it is walked as `Required`. References are not followed: a node
    // used as a whole child is judged by the engine in context, and a referenced
    // definition is walked when the loop reaches its own entry.
    for (&source, root) in input.ast_map {
        for def in root.defs() {
            if let Some(body) = def.body() {
                let located = Located::new(source, body);
                reporter.walk(&located, Participation::Required);
                // A resource ceiling tripped mid-construction: the verdicts that follow
                // would rest on an automaton we declined to finish, so stop and reject
                // the whole query as too complex rather than report anything dubious.
                if reporter.solver.is_too_complex() {
                    diagnose::report_too_complex(&located, reporter.diag);
                    return;
                }
            }
        }
    }
}

/// Walks definition bodies reporting impossible patterns: a concrete-kind node at a
/// required position the grammar can never build, and — when *no* branch of a required
/// alternation can match — each of those branches with its own reason. Holds the solver
/// and the diagnostic sink, so the recursion threads only the position and its
/// participation.
struct Reporter<'a, 'q> {
    solver: &'a mut SatisfiabilitySolver<'q>,
    diag: &'a mut Diagnostics,
}

impl Reporter<'_, '_> {
    /// Report what is impossible under `located` at `participation`. The descent
    /// crosses always-present wrappers, lowers into disjunctive branches and `?`/`*`
    /// bodies as `Deferred`, and stops at each node pattern, whose interior the
    /// engine judges whole.
    fn walk(&mut self, located: &Located<Pattern>, participation: Participation) {
        match located.node() {
            Pattern::NodePattern(node) => {
                let node = located.wrap(node.clone());
                if !participation.is_required() {
                    return;
                }
                if let Some(kind) = root_kind(self.solver.context(), &node) {
                    if !self.solver.satisfiable(&node, kind) {
                        diagnose::report(self.solver, &node, kind, self.diag);
                    }
                } else if is_wildcard_parent(&node) && !self.solver.wildcard_satisfiable(&node) {
                    diagnose::report_wildcard(&node, self.diag);
                }
            }
            Pattern::Union(_) | Pattern::Enum(_) => {
                let branches: Vec<Pattern> = located.node().children().collect();
                // A branch failing is normally excused by its siblings; but when every
                // branch is impossible the alternation is too, so promote them — each is
                // then reported with the reason it cannot match.
                let dead = participation.is_required()
                    && !branches.is_empty()
                    && branches
                        .iter()
                        .all(|branch| self.impossible(&located.wrap(branch.clone())));
                let branch_participation = if dead {
                    Participation::Required
                } else {
                    participation.inside_disjunction_branch()
                };
                for branch in &branches {
                    self.walk(&located.wrap(branch.clone()), branch_participation);
                }
            }
            Pattern::CapturedPattern(cap) => {
                if let Some(inner) = cap.inner() {
                    self.walk(&located.wrap(inner), participation);
                }
            }
            Pattern::FieldPattern(field) => {
                if let Some(value) = field.value() {
                    self.walk(&located.wrap(value), participation);
                }
            }
            Pattern::SeqPattern(seq) => {
                for child in seq.children() {
                    self.walk(&located.wrap(child), participation);
                }
            }
            Pattern::QuantifiedPattern(q) => {
                if let Some(inner) = q.inner() {
                    let inner_participation = participation.inside_quantifier_body(q);
                    self.walk(&located.wrap(inner), inner_participation);
                }
            }
            // A token always matches; a reference is walked at its own definition.
            Pattern::TokenPattern(_) | Pattern::DefRef(_) => {}
        }
    }

    /// Whether `located` provably cannot match any grammar tree — the cautious counterpart
    /// to satisfiability, used to decide an alternation is dead. It answers `true` only when
    /// impossibility is certain, never on doubt. An alternation is impossible only when every
    /// branch is; a sequence when any item is; a node when the solver says so; tokens,
    /// references, and optional bodies stay matchable.
    fn impossible(&mut self, located: &Located<Pattern>) -> bool {
        match located.node() {
            Pattern::NodePattern(node) => {
                let node = located.wrap(node.clone());
                match root_kind(self.solver.context(), &node) {
                    Some(kind) => !self.solver.satisfiable(&node, kind),
                    None if is_wildcard_parent(&node) => !self.solver.wildcard_satisfiable(&node),
                    None => false,
                }
            }
            Pattern::Union(_) | Pattern::Enum(_) => {
                let branches: Vec<Pattern> = located.node().children().collect();
                !branches.is_empty()
                    && branches
                        .iter()
                        .all(|branch| self.impossible(&located.wrap(branch.clone())))
            }
            Pattern::CapturedPattern(cap) => cap
                .inner()
                .is_some_and(|inner| self.impossible(&located.wrap(inner))),
            Pattern::FieldPattern(field) => field
                .value()
                .is_some_and(|value| self.impossible(&located.wrap(value))),
            Pattern::SeqPattern(seq) => seq
                .children()
                .any(|child| self.impossible(&located.wrap(child))),
            Pattern::QuantifiedPattern(q) => {
                !q.is_optional()
                    && q.inner()
                        .is_some_and(|inner| self.impossible(&located.wrap(inner)))
            }
            Pattern::TokenPattern(_) | Pattern::DefRef(_) => false,
        }
    }
}

/// A concrete-kind node pattern that must match, paired with its resolved kind.
struct Goal {
    node: Located<NodePattern>,
    kind: NodeKindId,
}

/// Collect the concrete-kind node patterns at `Required` positions reachable from
/// `located` without crossing into another node's child list. The walk descends
/// through the always-present wrappers (capture, field, sequence) and the
/// disjunctive ones (alternation, quantifier, lowering them to `Deferred`), but
/// stops at each node pattern — its subtree is judged whole by the engine.
fn collect_goals(
    located: &Located<Pattern>,
    participation: Participation,
    ctx: AutomatonContext<'_>,
    out: &mut Vec<Goal>,
) {
    match located.node() {
        Pattern::NodePattern(node) => {
            if !participation.is_required() {
                return;
            }
            let located_node = located.wrap(node.clone());
            if let Some(kind) = root_kind(ctx, &located_node) {
                out.push(Goal {
                    node: located_node,
                    kind,
                });
            }
        }
        // An anonymous literal at a goal position matches some token of the grammar.
        Pattern::TokenPattern(_) => {}
        Pattern::CapturedPattern(cap) => {
            if let Some(inner) = cap.inner() {
                collect_goals(&located.wrap(inner), participation, ctx, out);
            }
        }
        Pattern::FieldPattern(field) => {
            if let Some(value) = field.value() {
                collect_goals(&located.wrap(value), participation, ctx, out);
            }
        }
        Pattern::SeqPattern(seq) => {
            for child in seq.children() {
                collect_goals(&located.wrap(child), participation, ctx, out);
            }
        }
        Pattern::QuantifiedPattern(q) => {
            let Some(inner) = q.inner() else { return };
            let inner_participation = participation.inside_quantifier_body(q);
            collect_goals(&located.wrap(inner), inner_participation, ctx, out);
        }
        Pattern::Union(_) | Pattern::Enum(_) => {
            for branch in located.node().children() {
                collect_goals(
                    &located.wrap(branch),
                    participation.inside_disjunction_branch(),
                    ctx,
                    out,
                );
            }
        }
        // A reference's target is walked when the loop reaches its own definition.
        Pattern::DefRef(_) => {}
    }
}

/// The concrete named kind a node pattern roots a goal at, or `None` when the
/// position should be skipped (wildcard, supertype, error keyword, unresolved).
fn root_kind(ctx: AutomatonContext<'_>, located: &Located<NodePattern>) -> Option<NodeKindId> {
    let node = located.node();
    if node.is_any() {
        return None;
    }
    let type_token = node.kind_token()?;
    if matches!(
        type_token.kind(),
        SyntaxKind::KwError | SyntaxKind::KwMissing
    ) {
        return None;
    }
    let text = token_src(&type_token, ctx.content(located.source()));
    let id = ctx.grammar.resolve_named_node(text)?;
    // Query supertypes are rejected by the structural check; if one reaches here, skip it.
    if ctx.grammar.is_supertype(id) {
        return None;
    }
    Some(id)
}

/// A wildcard parent (`(_ …)` / `_ …`) that constrains children. It fixes no kind of
/// its own — so `root_kind` returns `None` — yet a child list still makes it possibly
/// impossible: satisfiability then asks whether *any* node kind takes those children.
/// A bare `(_)` constrains nothing and is always matchable, so it is excluded.
fn is_wildcard_parent(node: &Located<NodePattern>) -> bool {
    node.node().is_any() && node.node().items().next().is_some()
}
