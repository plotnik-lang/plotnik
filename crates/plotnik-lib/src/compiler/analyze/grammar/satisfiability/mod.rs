//! Grammar productions threaded through resolved query child automata. The
//! solver owns feasibility. Reporting follows failed constraints and retains source
//! syntax only for diagnostic wording and highlights.

mod automaton;
mod configurations;
mod diagnose;
mod engine;
mod facts;
mod shapes;

pub use engine::DEFAULT_SATISFIABILITY_WORK_BUDGET;

use crate::compiler::diagnostics::report::Diagnostics;
use crate::compiler::limits::SatisfiabilityLimits;
use crate::compiler::parse::ast::QuantifierKind;
use crate::core::NodeKindId;

use super::model::{KindConstraint, Model, PatternId, PatternKind};
use engine::SatisfiabilitySolver;

#[derive(Clone, Copy, Debug)]
struct AnalysisLimit;

pub(super) use diagnose::Context as ReportContext;

pub(super) fn check(ctx: ReportContext<'_>, limits: SatisfiabilityLimits, diag: &mut Diagnostics) {
    let mut solver = SatisfiabilitySolver::checking(ctx.grammar, ctx.model, limits);
    let mut failed = Vec::new();
    for pattern in ctx.model.definitions() {
        match solver.possible(pattern) {
            Ok(true) => {}
            Ok(false) => failed.push(pattern),
            Err(AnalysisLimit) => {
                if let Some(&impossible) = failed.first() {
                    diagnose::report_unexplained(ctx, impossible, diag);
                    return;
                }

                diagnose::report_too_complex(&ctx.model.pattern(pattern).origin, diag);
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
            Err(AnalysisLimit) => {
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
        if self.solver.possible(pattern)? {
            return Ok(());
        }

        let ctx = self.ctx;
        match &ctx.model.pattern(pattern).kind {
            PatternKind::Node(node) => {
                for child in &node.children {
                    if let Some(alternation) = self.dead_alternation_child(child.pattern)? {
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
            PatternKind::Field(_, inner) | PatternKind::Repeat(_, inner) => self.walk(*inner),
            PatternKind::Sequence(items) => {
                self.walk_children(items.iter().map(|item| item.pattern))
            }
            // Definitions are checked in their own right in declaration order.
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
        match &self.ctx.model.pattern(pattern).kind {
            PatternKind::Alternative(_) => Ok((!self.solver.possible(pattern)?).then_some(pattern)),
            PatternKind::Field(_, inner)
            | PatternKind::Repeat(QuantifierKind::OneOrMore, inner) => {
                self.dead_alternation_child(*inner)
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

fn collect_goals(pattern: PatternId, model: &Model, out: &mut Vec<Goal>) {
    let mut pending = vec![pattern];
    let mut visited = std::collections::HashSet::new();
    while let Some(pattern) = pending.pop() {
        if !visited.insert(pattern) {
            continue;
        }

        match &model.pattern(pattern).kind {
            PatternKind::Node(_) => {
                if let Some(goal) = Goal::from_pattern(model, pattern) {
                    out.push(goal);
                }
            }
            PatternKind::Field(_, inner)
            | PatternKind::Repeat(QuantifierKind::OneOrMore, inner) => pending.push(*inner),
            PatternKind::Sequence(items) => {
                pending.extend(items.iter().rev().map(|item| item.pattern))
            }
            PatternKind::Reference(definition) => pending.push(model.definition(*definition)),
            PatternKind::Repeat(_, _) | PatternKind::Alternative(_) | PatternKind::Leaf(_) => {}
        }
    }
}
