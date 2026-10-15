mod automaton;
mod budget;
mod configurations;
mod diagnose;
mod engine;
mod facts;
mod local;
mod shapes;

pub use engine::DEFAULT_SATISFIABILITY_WORK_BUDGET;

use crate::compiler::diagnostics::report::Diagnostics;
use crate::compiler::limits::SatisfiabilityLimits;
use crate::compiler::parse::ast::QuantifierKind;
use crate::core::NodeKindId;

use super::model::{KindConstraint, Model, PatternId, PatternKind};
use engine::SatisfiabilitySolver;

#[derive(Clone, Copy, Debug)]
pub(super) enum AnalysisLimit {
    ConstructionDepth,
    AutomatonStates,
    WorkBudget,
}

pub(super) use diagnose::Context as ReportContext;

pub(super) fn check(ctx: ReportContext<'_>, limits: SatisfiabilityLimits, diag: &mut Diagnostics) {
    let mut solver = SatisfiabilitySolver::checking(ctx.grammar, ctx.model, limits);
    let mut failed = Vec::new();
    for pattern in ctx.model.bodies() {
        match solver.possible(pattern) {
            Ok(true) => {}
            Ok(false) => failed.push(pattern),
            Err(limit) => {
                if let Some(&impossible) = failed.first() {
                    diagnose::report_unexplained(ctx, impossible, diag);
                    return;
                }

                diagnose::report_too_complex(&ctx.model.pattern(pattern).origin, limit, diag);
                return;
            }
        }
    }

    solver.begin_diagnostics(limits.work_budget);
    let mut reporter = Reporter {
        ctx,
        solver: &mut solver,
        diag,
        reported: diagnose::ReportedCulprits::default(),
    };
    for pattern in failed {
        match reporter.walk(pattern) {
            Ok(()) => {}
            Err(_) => {
                diagnose::report_unexplained(ctx, pattern, reporter.diag);
                return;
            }
        }
    }
}

struct Reporter<'a, 'q> {
    ctx: ReportContext<'q>,
    solver: &'a mut SatisfiabilitySolver<'q>,
    diag: &'a mut Diagnostics,
    reported: diagnose::ReportedCulprits,
}

impl Reporter<'_, '_> {
    fn walk(&mut self, pattern: PatternId) -> Result<(), AnalysisLimit> {
        self.solver.budget().charge()?;
        if self.solver.possible(pattern)? {
            return Ok(());
        }

        let ctx = self.ctx;
        match &ctx.model.pattern(pattern).kind {
            PatternKind::Node(node) => {
                for child in node.children.iter().filter_map(|item| item.pattern()) {
                    if let Some(alternation) = self.dead_alternation_child(child)? {
                        return self.walk(alternation);
                    }
                }
                let goal = Goal::from_pattern(ctx.model, pattern)
                    .expect("an impossible node has a concrete or wildcard goal");
                diagnose::report_goal(ctx, self.solver, goal, self.diag, &mut self.reported)
            }
            PatternKind::Alternative(alternatives) => {
                self.walk_children(alternatives.iter().copied())
            }
            PatternKind::Field(_, inner) => self.walk(*inner),
            PatternKind::Repeat(repeat) => self.walk(repeat.inner),
            PatternKind::Sequence(items) => {
                self.walk_children(items.iter().filter_map(|item| item.pattern()))
            }
            PatternKind::Reference(_) | PatternKind::Leaf(_) => Ok(()),
        }
    }

    fn walk_children(
        &mut self,
        children: impl Iterator<Item = PatternId>,
    ) -> Result<(), AnalysisLimit> {
        for child in children {
            self.walk(child)?;
        }
        Ok(())
    }

    fn dead_alternation_child(
        &mut self,
        pattern: PatternId,
    ) -> Result<Option<PatternId>, AnalysisLimit> {
        self.solver.budget().charge()?;
        match &self.ctx.model.pattern(pattern).kind {
            PatternKind::Alternative(_) => Ok((!self.solver.possible(pattern)?).then_some(pattern)),
            PatternKind::Field(_, inner) => self.dead_alternation_child(*inner),
            PatternKind::Repeat(repeat) if repeat.kind == QuantifierKind::OneOrMore => {
                self.dead_alternation_child(repeat.inner)
            }
            _ => Ok(None),
        }
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
    model: &Model,
    out: &mut Vec<Goal>,
    budget: &mut budget::Budget,
) -> Result<(), AnalysisLimit> {
    let mut pending = vec![pattern];
    let mut visited = std::collections::HashSet::new();
    while let Some(pattern) = pending.pop() {
        budget.charge()?;
        if !visited.insert(pattern) {
            continue;
        }

        match &model.pattern(pattern).kind {
            PatternKind::Node(_) => {
                if let Some(goal) = Goal::from_pattern(model, pattern) {
                    out.push(goal);
                }
            }
            PatternKind::Field(_, inner) => pending.push(*inner),
            PatternKind::Repeat(repeat) if repeat.kind == QuantifierKind::OneOrMore => {
                pending.push(repeat.inner)
            }
            PatternKind::Sequence(items) => {
                budget.spend(items.len() as u64)?;
                pending.extend(items.iter().rev().filter_map(|item| item.pattern()))
            }
            PatternKind::Reference(definition) => pending.push(model.definition(*definition)),
            PatternKind::Repeat(_) | PatternKind::Alternative(_) | PatternKind::Leaf(_) => {}
        }
    }
    Ok(())
}
