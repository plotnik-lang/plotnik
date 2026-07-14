//! Sequence, anchor, and grammar-arity satisfiability.
//!
//! The structural check (`../check.rs`) validates each query position in isolation —
//! kind exists, field is on the node, child kind is admissible. It is order-,
//! adjacency- and grammar-arity-blind, so `(function_declaration .! (identifier))` and
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
//! alternation alternative or quantified body, where a failure cannot be excused by a
//! sibling alternative or zero repetitions.

mod automaton;
mod diagnose;
mod engine;
mod facts;
mod state_set;

pub use engine::DEFAULT_SATISFIABILITY_WORK_BUDGET;

#[cfg(test)]
mod state_set_tests;

use indexmap::IndexMap;

use crate::compiler::analyze::Located;
use crate::compiler::analyze::names::SymbolTable;
use crate::compiler::diagnostics::report::Diagnostics;
use crate::compiler::diagnostics::source::{SourceId, SourceMap};
use crate::compiler::limits::SatisfiabilityLimits;
use crate::compiler::parse::ast::{self, NamedNodePattern, Pattern, Root, token_src};
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
    pub(super) limits: SatisfiabilityLimits,
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

    let mut solver = SatisfiabilitySolver::checking(ctx, input.limits);
    let anchor_probes = diagnose::AnchorProbes::new(&solver, input.limits.work_budget);
    let mut reporter = Reporter {
        solver: &mut solver,
        diag,
        anchor_probes,
        reported: diagnose::ReportedCulprits::default(),
    };

    // Each definition body must be matchable in its own right — the structural check's
    // stance — so it is walked as `Required`. References are not followed: a node
    // used as a whole child is judged by the engine in context, and a referenced
    // definition is walked when the loop reaches its own entry.
    for (&source, root) in input.ast_map {
        for def in root.defs() {
            let Some(body) = def.body() else {
                continue;
            };
            let located = Located::new(source, body);
            if reporter
                .walk(&located, Participation::Required)
                .should_stop()
            {
                return;
            }
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

/// Walks definition bodies reporting impossible patterns: a concrete-kind node at a
/// required position the grammar can never build, and — when *no* alternative of a required
/// alternation can match — each of those alternatives with its own reason. Holds the solver
/// and the diagnostic sink, so the recursion threads only the position and its
/// participation.
struct Reporter<'a, 'q> {
    solver: &'a mut SatisfiabilitySolver<'q>,
    diag: &'a mut Diagnostics,
    anchor_probes: diagnose::AnchorProbes<'q>,
    reported: diagnose::ReportedCulprits,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WalkOutcome {
    Continue,
    Stop,
}

impl WalkOutcome {
    fn should_stop(self) -> bool {
        matches!(self, Self::Stop)
    }
}

impl From<diagnose::ReportOutcome> for WalkOutcome {
    fn from(outcome: diagnose::ReportOutcome) -> Self {
        if outcome.should_stop() {
            Self::Stop
        } else {
            Self::Continue
        }
    }
}

impl Reporter<'_, '_> {
    /// Report what is impossible under `located` at `participation`. The descent
    /// crosses always-present wrappers, lowers into parallel alternatives and `?`/`*`
    /// bodies as `Deferred`, and stops at each node pattern, whose interior the
    /// engine judges whole.
    /// Returns `Stop` when reporting already emitted a terminal diagnostic; callers
    /// must then stop rather than spending fresh probe budgets on later nodes.
    fn walk(&mut self, located: &Located<Pattern>, participation: Participation) -> WalkOutcome {
        match located.node() {
            Pattern::NamedNodePattern(node) => {
                let node = located.wrap(node.clone());
                if !participation.is_required() {
                    return WalkOutcome::Continue;
                }
                let Some(goal) = Goal::from_node(self.solver.context(), node.clone()) else {
                    return WalkOutcome::Continue;
                };
                if !goal.is_impossible(self.solver) {
                    return WalkOutcome::Continue;
                }
                if let Some(outcome) = self.report_dead_child_alternation(&node) {
                    return outcome;
                }
                diagnose::report_goal(
                    self.solver,
                    goal,
                    self.diag,
                    &mut self.anchor_probes,
                    &mut self.reported,
                )
                .into()
            }
            Pattern::Alternation(_) => {
                // An alternative failing is normally excused by its siblings; but when every
                // alternative is impossible the alternation is too, so promote them — each is
                // then reported with the reason it cannot match.
                let dead = participation.is_required() && self.all_alternatives_impossible(located);
                let alternative_participation = if dead {
                    Participation::Required
                } else {
                    participation.inside_alternative()
                };
                self.walk_children(located, alternative_participation)
            }
            Pattern::CapturedPattern(cap) => {
                if let Some(inner) = cap.inner() {
                    return self.walk(&located.wrap(inner), participation);
                }
                WalkOutcome::Continue
            }
            Pattern::FieldPattern(field) => {
                if let Some(value) = field.value() {
                    return self.walk(&located.wrap(value), participation);
                }
                WalkOutcome::Continue
            }
            Pattern::SeqPattern(_) => self.walk_children(located, participation),
            Pattern::QuantifiedPattern(q) => {
                if let Some(inner) = q.inner() {
                    let inner_participation = participation.inside_quantifier_body(q);
                    return self.walk(&located.wrap(inner), inner_participation);
                }
                WalkOutcome::Continue
            }
            // Leaf patterns are checked by grammar binding; a reference is walked at its definition.
            Pattern::AnonymousNodePattern(_) | Pattern::NodeWildcard(_) | Pattern::DefRef(_) => {
                WalkOutcome::Continue
            }
        }
    }

    fn walk_children(
        &mut self,
        located: &Located<Pattern>,
        participation: Participation,
    ) -> WalkOutcome {
        for child in located.node().children() {
            let outcome = self.walk(&located.wrap(child), participation);
            if outcome.should_stop() {
                return outcome;
            }
        }
        WalkOutcome::Continue
    }

    fn report_dead_child_alternation(
        &mut self,
        node: &Located<NamedNodePattern>,
    ) -> Option<WalkOutcome> {
        for child in node.node().children() {
            let located = node.wrap(child);
            let Some(alternation) = self.dead_alternation_child(located) else {
                continue;
            };
            return Some(self.walk(&alternation, Participation::Required));
        }
        None
    }

    fn dead_alternation_child(&mut self, located: Located<Pattern>) -> Option<Located<Pattern>> {
        if matches!(located.node(), Pattern::Alternation(_)) {
            return self
                .all_alternatives_impossible(&located)
                .then_some(located);
        }
        match located.node() {
            Pattern::CapturedPattern(cap) => cap
                .inner()
                .and_then(|inner| self.dead_alternation_child(located.wrap(inner))),
            Pattern::FieldPattern(field) => field
                .value()
                .and_then(|value| self.dead_alternation_child(located.wrap(value))),
            Pattern::QuantifiedPattern(q) if !q.is_optional() => q
                .inner()
                .and_then(|inner| self.dead_alternation_child(located.wrap(inner))),
            _ => None,
        }
    }

    /// Whether `located` provably cannot match any grammar tree — the cautious counterpart
    /// to satisfiability, used to decide an alternation is dead. It answers `true` only when
    /// impossibility is certain, never on doubt. An alternation is impossible only when every
    /// alternative is; a sequence when any item is; a node when the solver says so; leaf patterns,
    /// references, and optional bodies stay matchable.
    fn impossible(&mut self, located: &Located<Pattern>) -> bool {
        match located.node() {
            Pattern::NamedNodePattern(node) => {
                let node = located.wrap(node.clone());
                Goal::from_node(self.solver.context(), node)
                    .is_some_and(|goal| goal.is_impossible(self.solver))
            }
            Pattern::Alternation(_) => self.all_alternatives_impossible(located),
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
            Pattern::DefRef(def_ref) => Goal::from_def_ref(self.solver.context(), def_ref)
                .is_some_and(|goal| goal.is_impossible(self.solver)),
            Pattern::AnonymousNodePattern(_) | Pattern::NodeWildcard(_) => false,
        }
    }

    fn all_alternatives_impossible(&mut self, located: &Located<Pattern>) -> bool {
        let mut saw_alternative = false;
        for alternative in located.node().children() {
            saw_alternative = true;
            if !self.impossible(&located.wrap(alternative)) {
                return false;
            }
        }
        saw_alternative
    }
}

/// A required node-position satisfiability goal.
enum Goal {
    Concrete {
        node: Located<NamedNodePattern>,
        kind: NodeKindId,
    },
    Wildcard {
        node: Located<NamedNodePattern>,
    },
}

impl Goal {
    fn from_node(ctx: AutomatonContext<'_>, node: Located<NamedNodePattern>) -> Option<Self> {
        if let Some(kind) = root_kind(ctx, &node) {
            return Some(Self::Concrete { node, kind });
        }
        is_wildcard_parent(&node).then_some(Self::Wildcard { node })
    }

    fn from_def_ref(ctx: AutomatonContext<'_>, def_ref: &ast::DefRef) -> Option<Self> {
        let name = def_ref.name()?;
        let target = ctx.symbol_table.located_definition(name.text())?;
        let Pattern::NamedNodePattern(node) = target.node() else {
            return None;
        };
        Self::from_node(ctx, target.wrap(node.clone()))
    }

    fn node(&self) -> &Located<NamedNodePattern> {
        match self {
            Self::Concrete { node, .. } | Self::Wildcard { node } => node,
        }
    }

    fn is_impossible(&self, solver: &mut SatisfiabilitySolver<'_>) -> bool {
        match self {
            Self::Concrete { node, kind } => !solver.satisfiable(node, *kind),
            Self::Wildcard { node } => !solver.wildcard_satisfiable(node),
        }
    }
}

/// Collect the node patterns at `Required` positions reachable from
/// `located` without crossing into another node's child list. The walk descends
/// through the always-present wrappers (capture, field, sequence) and the
/// disjunctive ones (alternation, quantifier, lowering them to `Deferred`), but
/// stops at each concrete or child-constraining wildcard node pattern — its subtree
/// is judged whole by the engine.
fn collect_goals(
    located: &Located<Pattern>,
    participation: Participation,
    ctx: AutomatonContext<'_>,
    out: &mut Vec<Goal>,
) {
    match located.node() {
        Pattern::NamedNodePattern(node) => {
            if !participation.is_required() {
                return;
            }
            let located_node = located.wrap(node.clone());
            if let Some(goal) = Goal::from_node(ctx, located_node) {
                out.push(goal);
            }
        }
        // A leaf pattern at a goal position is handled by grammar binding.
        Pattern::AnonymousNodePattern(_) | Pattern::NodeWildcard(_) => {}
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
        Pattern::Alternation(_) => {
            for alternative in located.node().children() {
                collect_goals(
                    &located.wrap(alternative),
                    participation.inside_alternative(),
                    ctx,
                    out,
                );
            }
        }
        Pattern::DefRef(def_ref) => {
            if participation.is_required()
                && let Some(goal) = Goal::from_def_ref(ctx, def_ref)
            {
                out.push(goal);
            }
        }
    }
}

/// The concrete named kind a node pattern roots a goal at, or `None` when the
/// position should be skipped (wildcard, supertype, error keyword, unresolved).
fn root_kind(ctx: AutomatonContext<'_>, located: &Located<NamedNodePattern>) -> Option<NodeKindId> {
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
/// its own — so `root_kind` returns `None` — yet child-structure constraints still
/// make it possibly impossible: satisfiability then asks whether *any* node kind can
/// realize them. A bare `(_)` constrains nothing and is always matchable, so it is
/// excluded.
fn is_wildcard_parent(node: &Located<NamedNodePattern>) -> bool {
    node.node().is_any() && node_constrains_children(node.node())
}

fn node_constrains_children(node: &NamedNodePattern) -> bool {
    node.items().next().is_some()
        || node
            .syntax()
            .children()
            .any(|child| ast::NegatedField::cast(child).is_some())
}
