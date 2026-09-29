//! Grammar productions threaded through resolved query child automata. The
//! structural checker remains separate until checkpoint 3, and diagnostic wording
//! still uses source syntax until checkpoint 4.

mod automaton;
mod configurations;
mod diagnose;
mod engine;
mod facts;

pub use engine::DEFAULT_SATISFIABILITY_WORK_BUDGET;

use crate::compiler::diagnostics::report::Diagnostics;
use crate::compiler::limits::SatisfiabilityLimits;
use crate::compiler::parse::ast::QuantifierKind;
use crate::core::NodeKindId;

use super::model::{KindConstraint, Model, PatternId, PatternKind};
use super::participation::Participation;
use engine::SatisfiabilitySolver;

#[derive(Clone, Copy, Debug)]
struct AnalysisLimit;

pub(super) use diagnose::Context as ReportContext;

pub(super) fn check(ctx: ReportContext<'_>, limits: SatisfiabilityLimits, diag: &mut Diagnostics) {
    if ctx.grammar.structure().variables().is_empty() {
        return;
    }

    let mut solver = SatisfiabilitySolver::checking(ctx.grammar, ctx.model, limits);
    let mut reporter = Reporter {
        ctx,
        solver: &mut solver,
        diag,
        diagnostic_budget: limits.work_budget,
        reported: diagnose::ReportedCulprits::default(),
    };

    for pattern in ctx.model.definitions() {
        match reporter.walk(pattern, Participation::Required) {
            Ok(WalkOutcome::Continue) => {}
            Ok(WalkOutcome::Stop) => return,
            Err(AnalysisLimit) => {
                diagnose::report_too_complex(&ctx.model.pattern(pattern).origin, reporter.diag);
                return;
            }
        }
    }
}

struct Reporter<'a, 'q> {
    ctx: ReportContext<'q>,
    solver: &'a mut SatisfiabilitySolver<'q>,
    diag: &'a mut Diagnostics,
    diagnostic_budget: u64,
    reported: diagnose::ReportedCulprits,
}

enum WalkOutcome {
    Continue,
    Stop,
}

impl WalkOutcome {
    fn should_stop(&self) -> bool {
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
    fn walk(
        &mut self,
        pattern: PatternId,
        participation: Participation,
    ) -> Result<WalkOutcome, AnalysisLimit> {
        let ctx = self.ctx;
        match &ctx.model.pattern(pattern).kind {
            PatternKind::Node(node) => {
                if !participation.is_required() {
                    return Ok(WalkOutcome::Continue);
                }
                let Some(goal) = Goal::from_pattern(ctx.model, pattern) else {
                    return Ok(WalkOutcome::Continue);
                };
                if !goal.is_impossible(self.solver)? {
                    return Ok(WalkOutcome::Continue);
                }

                for child in &node.children {
                    if let Some(alternation) = self.dead_alternation_child(child.pattern)? {
                        return self.walk(alternation, Participation::Required);
                    }
                }
                diagnose::report_goal(
                    ctx,
                    self.solver,
                    goal,
                    self.diag,
                    &mut self.diagnostic_budget,
                    &mut self.reported,
                )
                .map(Into::into)
            }
            PatternKind::Alternative(alternatives) => {
                let dead = participation.is_required() && self.all_impossible(alternatives)?;
                let participation = if dead {
                    Participation::Required
                } else {
                    Participation::Deferred
                };
                self.walk_children(alternatives.iter().copied(), participation)
            }
            PatternKind::Field(_, inner) => self.walk(*inner, participation),
            PatternKind::Sequence(items) => {
                self.walk_children(items.iter().map(|item| item.pattern), participation)
            }
            PatternKind::Repeat(kind, inner) => {
                let participation = if *kind == QuantifierKind::OneOrMore {
                    participation
                } else {
                    Participation::Deferred
                };
                self.walk(*inner, participation)
            }
            // Each definition is checked separately, in declaration order.
            PatternKind::Leaf(_) | PatternKind::Reference(_) => Ok(WalkOutcome::Continue),
        }
    }

    fn walk_children(
        &mut self,
        children: impl Iterator<Item = PatternId>,
        participation: Participation,
    ) -> Result<WalkOutcome, AnalysisLimit> {
        for child in children {
            let outcome = self.walk(child, participation)?;
            if outcome.should_stop() {
                return Ok(outcome);
            }
        }
        Ok(WalkOutcome::Continue)
    }

    fn dead_alternation_child(
        &mut self,
        pattern: PatternId,
    ) -> Result<Option<PatternId>, AnalysisLimit> {
        let model = self.ctx.model;
        match &model.pattern(pattern).kind {
            PatternKind::Alternative(alternatives) => {
                Ok(self.all_impossible(alternatives)?.then_some(pattern))
            }
            PatternKind::Field(_, inner)
            | PatternKind::Repeat(QuantifierKind::OneOrMore, inner) => {
                self.dead_alternation_child(*inner)
            }
            _ => Ok(None),
        }
    }

    fn impossible(&mut self, pattern: PatternId) -> Result<bool, AnalysisLimit> {
        let ctx = self.ctx;
        match &ctx.model.pattern(pattern).kind {
            PatternKind::Node(_) => match Goal::from_pattern(ctx.model, pattern) {
                Some(goal) => goal.is_impossible(self.solver),
                None => Ok(false),
            },
            PatternKind::Alternative(alternatives) => self.all_impossible(alternatives),
            PatternKind::Field(_, inner)
            | PatternKind::Repeat(QuantifierKind::OneOrMore, inner) => self.impossible(*inner),
            PatternKind::Sequence(items) => {
                for item in items {
                    if self.impossible(item.pattern)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            PatternKind::Reference(definition) => {
                match Goal::from_pattern(ctx.model, ctx.model.definition(*definition)) {
                    Some(goal) => goal.is_impossible(self.solver),
                    None => Ok(false),
                }
            }
            PatternKind::Leaf(_) | PatternKind::Repeat(_, _) => Ok(false),
        }
    }

    fn all_impossible(&mut self, alternatives: &[PatternId]) -> Result<bool, AnalysisLimit> {
        for &alternative in alternatives {
            if !self.impossible(alternative)? {
                return Ok(false);
            }
        }
        Ok(!alternatives.is_empty())
    }
}

#[derive(Clone, Copy)]
enum Goal {
    Concrete {
        pattern: PatternId,
        kind: NodeKindId,
    },
    Wildcard {
        pattern: PatternId,
    },
}

impl Goal {
    fn from_pattern(model: &Model, pattern: PatternId) -> Option<Self> {
        let PatternKind::Node(node) = &model.pattern(pattern).kind else {
            return None;
        };
        match node.kind {
            KindConstraint::Exact(kind) => Some(Self::Concrete { pattern, kind }),
            KindConstraint::AnyNamed if node.constrains_children() => {
                Some(Self::Wildcard { pattern })
            }
            _ => None,
        }
    }

    fn pattern(self) -> PatternId {
        match self {
            Self::Concrete { pattern, .. } | Self::Wildcard { pattern } => pattern,
        }
    }

    fn is_impossible(self, solver: &mut SatisfiabilitySolver<'_>) -> Result<bool, AnalysisLimit> {
        let matches = match self {
            Self::Concrete { pattern, kind } => solver.satisfiable(pattern, kind)?,
            Self::Wildcard { pattern } => solver.wildcard_satisfiable(pattern)?,
        };
        Ok(!matches)
    }
}

fn collect_goals(
    pattern: PatternId,
    participation: Participation,
    model: &Model,
    out: &mut Vec<Goal>,
) {
    match &model.pattern(pattern).kind {
        PatternKind::Node(_) => {
            if participation.is_required()
                && let Some(goal) = Goal::from_pattern(model, pattern)
            {
                out.push(goal);
            }
        }
        PatternKind::Field(_, inner) => collect_goals(*inner, participation, model, out),
        PatternKind::Sequence(items) => {
            for item in items {
                collect_goals(item.pattern, participation, model, out);
            }
        }
        PatternKind::Repeat(kind, inner) => {
            let participation = if *kind == QuantifierKind::OneOrMore {
                participation
            } else {
                Participation::Deferred
            };
            collect_goals(*inner, participation, model, out);
        }
        PatternKind::Alternative(alternatives) => {
            for &alternative in alternatives {
                collect_goals(alternative, Participation::Deferred, model, out);
            }
        }
        PatternKind::Reference(definition) => {
            if participation.is_required()
                && let Some(goal) = Goal::from_pattern(model, model.definition(*definition))
            {
                out.push(goal);
            }
        }
        PatternKind::Leaf(_) => {}
    }
}
