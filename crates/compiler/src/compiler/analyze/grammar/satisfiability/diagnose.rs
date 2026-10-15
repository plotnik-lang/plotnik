use std::collections::HashSet;

use crate::compiler::analyze::Located;
use crate::compiler::analyze::boundary::PendingAnchor;
use crate::compiler::diagnostics::report::{DiagnosticKind, Diagnostics, Span};
use crate::compiler::parse::ast::{self, NamedNodePattern, Pattern};
use crate::core::grammar::Grammar;
use crate::core::{NodeFieldId, NodeKindId};

use super::super::format::format_kind;
use super::super::model::{Item, KindConstraint, Model, PatternId, PatternKind};
use super::AnalysisLimit;
use super::automaton::{ChildAutomaton, ChildMatcher};
use super::budget::Budget;
use super::engine::SatisfiabilitySolver;
use super::shapes::Child;
use super::{Goal, collect_goals};

#[derive(Clone, Copy)]
pub(in crate::compiler::analyze::grammar) struct Context<'a> {
    pub(in crate::compiler::analyze::grammar) grammar: &'a Grammar,
    pub(in crate::compiler::analyze::grammar) model: &'a Model,
}

#[derive(Default)]
pub(super) struct ReportedCulprits {
    nodes: HashSet<PatternId>,
}

impl ReportedCulprits {
    fn insert(&mut self, culprit: &Goal) -> bool {
        self.nodes.insert(culprit.pattern())
    }
}

pub(super) fn report_goal<'q>(
    ctx: Context<'q>,
    solver: &mut SatisfiabilitySolver<'q>,
    goal: Goal,
    diag: &mut Diagnostics,
    reported: &mut ReportedCulprits,
) -> Result<(), AnalysisLimit> {
    if super::local::LocalDiagnostics::report(
        ctx.grammar,
        ctx.model,
        solver,
        goal.pattern(),
        diag,
        &mut reported.nodes,
    )? {
        return Ok(());
    }

    let culprit = locate(ctx.model, solver, goal)?;
    if !reported.insert(&culprit) {
        return Ok(());
    }
    report_culprit(ctx, solver, culprit, diag)
}

fn report_culprit(
    ctx: Context<'_>,
    solver: &mut SatisfiabilitySolver,
    culprit: Goal,
    diag: &mut Diagnostics,
) -> Result<(), AnalysisLimit> {
    let culprit = match ConcreteCulprit::from_goal(ctx.model, culprit) {
        Ok(culprit) => culprit,
        Err(pattern) => {
            emit_wildcard_failure(ctx.model, pattern, diag);
            return Ok(());
        }
    };

    if report_negated_fields(ctx, solver, &culprit, diag)? {
        return Ok(());
    }

    if let Some((field, spans)) = repeated_single_field(ctx, solver, &culprit)? {
        emit_repeated_field_failure(ctx, &culprit, &field, &spans, diag);
        return Ok(());
    }

    if report_nested_constraints(ctx, solver, &culprit, diag)? {
        return Ok(());
    }

    if !report_anchors(ctx, solver, &culprit, diag)? {
        emit_arrangement_failure(ctx, solver, &culprit, diag)?;
    }
    Ok(())
}

struct ConcreteCulprit {
    pattern: PatternId,
    node: Located<NamedNodePattern>,
    kind: NodeKindId,
}

impl ConcreteCulprit {
    fn new(model: &Model, pattern: PatternId, kind: NodeKindId) -> Self {
        Self {
            pattern,
            node: model.node_origin(pattern),
            kind,
        }
    }

    fn from_goal(model: &Model, goal: Goal) -> Result<Self, PatternId> {
        match goal {
            Goal::Concrete { pattern, kind } => Ok(Self::new(model, pattern, kind)),
            Goal::Wildcard { pattern } => Err(pattern),
        }
    }

    fn span(&self) -> Span {
        kind_span(&self.node)
    }

    fn kind_name(&self, ctx: Context<'_>) -> String {
        render_kind(ctx, self.kind)
    }
}

fn emit_wildcard_failure(model: &Model, pattern: PatternId, diag: &mut Diagnostics) {
    diag.report(
        DiagnosticKind::UnsatisfiablePattern,
        constraints_span(model, pattern),
    )
    .detail("no named node in the grammar satisfies these child constraints".to_string())
    .emit();
}

pub(super) fn report_too_complex(
    body: &Located<Pattern>,
    limit: AnalysisLimit,
    diag: &mut Diagnostics,
) {
    let span = Span::new(body.source(), body.node().syntax().text_range());
    let (reason, hint) = match limit {
        AnalysisLimit::ConstructionDepth => (
            "grammar analysis exceeded its pattern construction depth limit",
            "reduce nesting in child patterns and definition references",
        ),
        AnalysisLimit::AutomatonStates => (
            "grammar analysis exceeded its expanded child-pattern size limit",
            "simplify groups or definitions that duplicate sibling patterns",
        ),
        AnalysisLimit::WorkBudget => (
            "grammar analysis exhausted its work budget",
            "reduce the child constraints or increase the grammar-analysis work budget",
        ),
    };
    diag.report(DiagnosticKind::QueryTooComplex, span)
        .detail(reason)
        .hint(hint)
        .emit();
}

fn repeated_single_field(
    ctx: Context<'_>,
    solver: &mut SatisfiabilitySolver,
    culprit: &ConcreteCulprit,
) -> Result<Option<(String, Vec<Span>)>, AnalysisLimit> {
    let mut fields: Vec<(NodeFieldId, Vec<Span>)> = Vec::new();
    for child in query_child_demands(ctx, culprit.pattern, solver.budget())? {
        let ChildDemand::Field(id) = child.demand else {
            continue;
        };
        solver.budget().spend(fields.len() as u64 + 1)?;
        match fields.iter_mut().find(|(seen, _)| *seen == id) {
            Some((_, spans)) => spans.push(child.span),
            None => fields.push((id, vec![child.span])),
        }
    }
    for (field, spans) in fields {
        if spans.len() < 2 {
            continue;
        }
        let matcher = ChildDemand::Field(field).matcher();
        let probe = ChildAutomaton::ordered_pair(matcher, matcher, solver.budget())?;
        if solver.probe(culprit.kind, probe)? {
            continue;
        }
        return Ok(Some((
            ctx.grammar
                .field_name(field)
                .expect("resolved field has a name")
                .to_string(),
            spans,
        )));
    }
    Ok(None)
}

fn emit_repeated_field_failure(
    ctx: Context<'_>,
    culprit: &ConcreteCulprit,
    field: &str,
    spans: &[Span],
    diag: &mut Diagnostics,
) {
    let count = spans.len();
    let kind_name = culprit.kind_name(ctx);
    let article = indefinite_article(&kind_name);
    let detail = format!(
        "{article} {kind_name} has at most one `{field}`, but this pattern requires it at least {count} times"
    );
    diag.report(DiagnosticKind::UnsatisfiablePattern, spans[1])
        .detail(detail)
        .related_to(spans[0], format!("first `{field}:` child"))
        .related_to(culprit.span(), format!("inside {kind_name}"))
        .hint(format!(
            "use one `{field}:` pattern for this single-valued field"
        ))
        .emit();
}

fn report_negated_fields(
    ctx: Context<'_>,
    solver: &mut SatisfiabilitySolver,
    culprit: &ConcreteCulprit,
    diag: &mut Diagnostics,
) -> Result<bool, AnalysisLimit> {
    let mut reported = false;
    let node = ctx.model.node(culprit.pattern);
    for field in &node.negated_fields {
        let name = ctx
            .grammar
            .field_name(field.id)
            .expect("resolved field has a name");
        let span = field.span;
        let kind = culprit.kind_name(ctx);
        let probe = ChildAutomaton::without_field(field.id, solver.budget())?;
        if !solver.probe(culprit.kind, probe)? {
            diag.report(DiagnosticKind::NegatedRequiredField, span)
                .detail(name)
                .related_to(culprit.span(), format!("on {kind}"))
                .hint(format!("`-{name}` requires `{name}` to be absent, but every {kind} has one. Drop `-{name}`"))
                .emit();
            reported = true;
            continue;
        }

        let mut relaxed = solver.copy_automaton(culprit.pattern)?;
        relaxed.allow_field(field.id, solver.budget())?;
        if solver.probe(culprit.kind, relaxed)? {
            let others = cover_spans(
                constraint_spans(ctx.model, culprit.pattern).filter(|&other| other != span),
            )
            .expect("a conditional field conflict has other constraints");
            diag.report(DiagnosticKind::UnsatisfiablePattern, span)
                .detail(format!("`-{name}` requires `{name}` to be absent, but the remaining constraints on {kind} require it to be present"))
                .related_to(others, "these constraints require the field to be present")
                .hint(format!("drop `-{name}` to allow the remaining constraints"))
                .emit();
            reported = true;
        }
    }
    Ok(reported)
}

fn locate(
    model: &Model,
    solver: &mut SatisfiabilitySolver,
    mut goal: Goal,
) -> Result<Goal, AnalysisLimit> {
    let mut visited = HashSet::new();
    while visited.insert(goal.pattern()) {
        solver.budget().charge()?;
        let mut children = Vec::new();
        for child in model
            .node(goal.pattern())
            .children
            .iter()
            .filter_map(|item| item.pattern())
        {
            collect_goals(child, model, &mut children, solver.budget())?;
        }
        let mut failed = None;
        for child in children {
            if child.is_impossible(solver)? {
                failed = Some(child);
                break;
            }
        }
        let Some(child) = failed else {
            return Ok(goal);
        };
        goal = child;
    }
    Ok(goal)
}

fn report_anchors(
    ctx: Context<'_>,
    solver: &mut SatisfiabilitySolver,
    culprit: &ConcreteCulprit,
    diag: &mut Diagnostics,
) -> Result<bool, AnalysisLimit> {
    let original = solver.copy_automaton(culprit.pattern)?;
    let mut anchors: Vec<(Span, Vec<u32>)> = Vec::new();
    let mut relaxed = original.copy(solver.budget())?;
    for state in 0..original.state_count() as u32 {
        solver.budget().spend(anchors.len() as u64 + 1)?;
        let Some((_, span)) = original.anchor(state) else {
            continue;
        };
        if let Some((_, states)) = anchors.iter_mut().find(|(seen, _)| *seen == span) {
            states.push(state);
        } else {
            anchors.push((span, vec![state]));
        }
    }
    if anchors.is_empty() {
        return Ok(false);
    }

    relaxed.clear_anchors(solver.budget())?;
    if !solver.probe(culprit.kind, relaxed)? {
        return Ok(false);
    }

    let kind = culprit.kind_name(ctx);
    let shapes = solver.child_shapes(culprit.kind)?;
    for (span, states) in &anchors {
        let trial = original.copy(solver.budget()).and_then(|mut removed| {
            removed.remove_anchor(*span, solver.budget())?;
            solver.probe(culprit.kind, removed)
        });
        match trial {
            Ok(false) => continue,
            Err(_) => break,
            Ok(true) => {}
        }

        let children = &ctx.model.node(culprit.pattern).children;
        let position = children
            .iter()
            .position(|item| matches!(item, Item::Anchor(_, source) if source == span))
            .map_or("", |index| {
                let before = children[..index]
                    .iter()
                    .any(|item| item.pattern().is_some());
                let after = children[index + 1..]
                    .iter()
                    .any(|item| item.pattern().is_some());
                match (before, after) {
                    (false, false) => "childless ",
                    (false, true) => "leading ",
                    (true, false) => "trailing ",
                    (true, true) => "interior ",
                }
            });
        let exact = states.iter().any(|&state| {
            original
                .anchor(state)
                .is_some_and(|(anchor, _)| anchor == PendingAnchor::Exact)
        });
        let anchor = if exact {
            "exact anchor (`.!`)"
        } else {
            "soft anchor (`.`)"
        };
        let mut builder = diag
            .report(DiagnosticKind::UnsatisfiablePattern, *span)
            .detail(format!(
                "the {position}{anchor} prevents these children from matching {kind}"
            ))
            .related_to(culprit.span(), format!("inside {kind}"));
        if let Some(shapes) = &shapes {
            builder = builder.hint(describe_shapes(ctx, &kind, shapes));
        }
        builder
            .fix("remove this anchor to allow these child patterns", "")
            .emit();
        return Ok(true);
    }

    let mut builder = diag
        .report(DiagnosticKind::UnsatisfiablePattern, anchors[0].0)
        .detail(format!(
            "these anchors prevent the child patterns from matching {kind}"
        ))
        .related_to(culprit.span(), format!("inside {kind}"));
    for (span, _) in &anchors[1..] {
        builder = builder.related_to(*span, "also constrains the child positions");
    }
    if let Some(shapes) = &shapes {
        builder = builder.hint(describe_shapes(ctx, &kind, shapes));
    }
    builder
        .hint("remove the highlighted anchors to allow these child patterns")
        .emit();
    Ok(true)
}

fn emit_arrangement_failure(
    ctx: Context<'_>,
    solver: &mut SatisfiabilitySolver,
    culprit: &ConcreteCulprit,
    diag: &mut Diagnostics,
) -> Result<(), AnalysisLimit> {
    if let Some(order) = order_diagnosis(ctx, solver, culprit)? {
        emit_order_failure(ctx, culprit, order, diag);
        return Ok(());
    }

    let shapes = solver.child_shapes(culprit.kind)?;
    let demands = query_child_demands(ctx, culprit.pattern, solver.budget())?;
    if let Some(shapes) = &shapes
        && report_child_count(ctx, culprit, &demands, shapes, solver.budget(), diag)?
    {
        return Ok(());
    }

    let kind = culprit.kind_name(ctx);
    let mut builder = diag
        .report(
            DiagnosticKind::UnsatisfiablePattern,
            constraints_span(ctx.model, culprit.pattern),
        )
        .detail(format!(
            "these child constraints cannot be satisfied together in {kind}"
        ))
        .related_to(culprit.span(), format!("inside {kind}"));
    if let Some(shapes) = &shapes
        && !demands.is_empty()
        && shapes.iter().all(|shape| !admits_demands(shape, &demands))
    {
        builder = builder.hint(describe_shapes(ctx, &kind, shapes));
    }
    builder.emit();
    Ok(())
}

fn admits_demands(shape: &[Child], demands: &[QueryChild]) -> bool {
    let mut remaining = shape.iter();
    demands
        .iter()
        .all(|demand| remaining.any(|child| demand.demand.accepts(child)))
}

fn report_child_count(
    ctx: Context<'_>,
    culprit: &ConcreteCulprit,
    demands: &[QueryChild],
    shapes: &[Vec<Child>],
    budget: &mut Budget,
    diag: &mut Diagnostics,
) -> Result<bool, AnalysisLimit> {
    for (index, child) in demands.iter().enumerate() {
        budget.charge()?;
        if matches!(child.demand, ChildDemand::Kind(kind) if ctx.grammar.is_extra(kind)) {
            continue;
        }
        budget.spend(
            demands.len() as u64 * 2 + shapes.iter().map(|shape| shape.len() as u64).sum::<u64>(),
        )?;
        let required: Vec<_> = demands
            .iter()
            .filter(|other| other.demand == child.demand)
            .collect();
        if demands[..index]
            .iter()
            .any(|other| other.demand == child.demand)
        {
            continue;
        }
        let maximum = shapes
            .iter()
            .map(|shape| {
                shape
                    .iter()
                    .filter(|item| child.demand.accepts(item))
                    .count()
            })
            .max()
            .expect("complete shape descriptions are nonempty");
        if required.len() <= maximum {
            continue;
        }

        let parent = culprit.kind_name(ctx);
        let kind = child.demand.label(ctx);
        let detail = if maximum == 0 {
            format!("{parent} cannot contain a {kind} child")
        } else {
            format!(
                "{parent} has at most {maximum} {kind} child{}, but this pattern requires {}",
                if maximum == 1 { "" } else { "ren" },
                required.len()
            )
        };
        let mut builder = diag
            .report(DiagnosticKind::UnsatisfiablePattern, required[maximum].span)
            .detail(detail)
            .related_to(culprit.span(), format!("inside {parent}"));
        for earlier in required.iter().take(maximum) {
            builder = builder.related_to(earlier.span, format!("also requires {kind}"));
        }
        builder.hint(describe_shapes(ctx, &parent, shapes)).emit();
        return Ok(true);
    }
    Ok(false)
}

fn describe_shapes(ctx: Context<'_>, kind: &str, shapes: &[Vec<Child>]) -> String {
    let sequences: Vec<_> = shapes
        .iter()
        .map(|shape| {
            if shape.is_empty() {
                return "no children".to_string();
            }
            let children: Vec<_> = shape
                .iter()
                .map(|child| {
                    let name = format_kind(ctx.grammar, child.kind);
                    match child.field {
                        Some(field) => format!(
                            "{}: {name}",
                            ctx.grammar
                                .field_name(field)
                                .expect("retained field has a name")
                        ),
                        None => name,
                    }
                })
                .collect();
            format!("`{}`", children.join(" "))
        })
        .collect();
    let extras = if ctx.grammar.extra_node_kinds().is_empty() {
        ""
    } else {
        " (apart from extras)"
    };
    format!("{kind} allows {}{extras}", sequences.join(" or "))
}

fn constraint_spans(model: &Model, pattern: PatternId) -> impl Iterator<Item = Span> + '_ {
    let node = model.node(pattern);
    node.children
        .iter()
        .map(|item| match item {
            Item::Pattern(child) => {
                let origin = &model.pattern(*child).origin;
                origin.span_of(origin.node().text_range())
            }
            Item::Anchor(_, span) => *span,
        })
        .chain(node.negated_fields.iter().map(|field| field.span))
}

fn cover_spans(spans: impl Iterator<Item = Span>) -> Option<Span> {
    spans.reduce(|left, right| Span::new(left.source, left.range.cover(right.range)))
}

fn constraints_span(model: &Model, pattern: PatternId) -> Span {
    cover_spans(constraint_spans(model, pattern))
        .unwrap_or_else(|| kind_span(&model.node_origin(pattern)))
}

fn report_nested_constraints(
    ctx: Context<'_>,
    solver: &mut SatisfiabilitySolver,
    culprit: &ConcreteCulprit,
    diag: &mut Diagnostics,
) -> Result<bool, AnalysisLimit> {
    let original = solver.copy_automaton(culprit.pattern)?;
    let mut seen = HashSet::new();
    for nested in original.nested_patterns() {
        solver.budget().charge()?;
        if !seen.insert(nested) {
            continue;
        }
        let mut relaxed = original.copy(solver.budget())?;
        relaxed.allow_nested(nested, solver.budget())?;
        if !solver.probe(culprit.kind, relaxed)? {
            continue;
        }

        let parent = culprit.kind_name(ctx);
        let resolved = ctx.model.node(nested);
        if resolved.children.is_empty() && resolved.negated_fields.len() == 1 {
            let field = resolved.negated_fields[0];
            let name = ctx
                .grammar
                .field_name(field.id)
                .expect("resolved field has a name");
            diag.report(DiagnosticKind::UnsatisfiablePattern, field.span)
                .detail(format!(
                    "`-{name}` excludes the field required at this child position in {parent}"
                ))
                .related_to(culprit.span(), format!("inside {parent}"))
                .hint(format!("drop `-{name}` to allow this child position"))
                .emit();
            return Ok(true);
        }

        let childless_anchor = if resolved.negated_fields.is_empty()
            && resolved
                .children
                .iter()
                .all(|item| item.pattern().is_none())
        {
            resolved.children.iter().find_map(|item| match item {
                Item::Anchor(PendingAnchor::Exact, span) => Some(*span),
                _ => None,
            })
        } else {
            None
        };
        let empty = childless_anchor.is_some();
        let span = childless_anchor.unwrap_or_else(|| constraints_span(ctx.model, nested));
        let detail = if empty {
            format!(
                "this pattern requires a childless node, but this position in {parent} needs a node with children"
            )
        } else if resolved.negated_fields.is_empty() {
            format!("these nested child constraints cannot be satisfied inside {parent}")
        } else {
            format!(
                "these nested child or absent-field constraints cannot be satisfied inside {parent}"
            )
        };
        let mut builder = diag
            .report(DiagnosticKind::UnsatisfiablePattern, span)
            .detail(detail)
            .related_to(culprit.span(), format!("inside {parent}"));
        if let KindConstraint::Exact(kind) = resolved.kind {
            let name = ctx
                .grammar
                .node_kind(kind)
                .expect("resolved kind has a name");
            builder = builder.hint(format!(
                "match `({name})` here without its child or negated-field constraints"
            ));
        }
        builder.emit();
        return Ok(true);
    }
    Ok(false)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChildDemand {
    Field(NodeFieldId),
    Kind(NodeKindId),
}

#[derive(Clone, Debug)]
struct QueryChild {
    demand: ChildDemand,
    span: Span,
}

struct OrderDiagnosis {
    before: QueryChild,
    after: QueryChild,
}

fn order_diagnosis(
    ctx: Context<'_>,
    solver: &mut SatisfiabilitySolver,
    culprit: &ConcreteCulprit,
) -> Result<Option<OrderDiagnosis>, AnalysisLimit> {
    let query = query_child_demands(ctx, culprit.pattern, solver.budget())?;
    for (index, before) in query.iter().enumerate() {
        for after in &query[index + 1..] {
            solver.budget().charge()?;
            if before.demand == after.demand {
                continue;
            }

            let requested = ChildAutomaton::ordered_pair(
                before.demand.matcher(),
                after.demand.matcher(),
                solver.budget(),
            )?;
            if solver.probe(culprit.kind, requested)? {
                continue;
            }

            let reversed = ChildAutomaton::ordered_pair(
                after.demand.matcher(),
                before.demand.matcher(),
                solver.budget(),
            )?;
            if solver.probe(culprit.kind, reversed)? {
                return Ok(Some(OrderDiagnosis {
                    before: after.clone(),
                    after: before.clone(),
                }));
            }
        }
    }
    Ok(None)
}

fn query_child_demands(
    ctx: Context<'_>,
    node: PatternId,
    budget: &mut Budget,
) -> Result<Vec<QueryChild>, AnalysisLimit> {
    let mut children = Vec::new();
    budget.spend(ctx.model.node(node).children.len() as u64)?;
    let mut pending: Vec<_> = ctx
        .model
        .node(node)
        .children
        .iter()
        .rev()
        .filter_map(|item| item.pattern())
        .map(|pattern| (pattern, HashSet::new()))
        .collect();
    while let Some((pattern, mut references)) = pending.pop() {
        budget.charge()?;
        let demand = match &ctx.model.pattern(pattern).kind {
            PatternKind::Field(field, inner) if guaranteed_child(ctx.model, *inner, budget)? => {
                ChildDemand::Field(*field)
            }
            PatternKind::Node(node) => match node.kind {
                KindConstraint::Exact(kind) => ChildDemand::Kind(kind),
                _ => continue,
            },
            PatternKind::Leaf(KindConstraint::Exact(kind)) => ChildDemand::Kind(*kind),
            PatternKind::Reference(definition) => {
                if references.insert(*definition) {
                    pending.push((ctx.model.definition(*definition), references));
                }
                continue;
            }
            PatternKind::Sequence(items) => {
                budget.spend(items.len() as u64 * (references.len() as u64 + 1))?;
                pending.extend(
                    items
                        .iter()
                        .rev()
                        .filter_map(|item| item.pattern())
                        .map(|pattern| (pattern, references.clone())),
                );
                continue;
            }
            PatternKind::Repeat(repeat) if repeat.kind == ast::QuantifierKind::OneOrMore => {
                pending.push((repeat.inner, references));
                continue;
            }
            _ => continue,
        };
        children.push(QueryChild {
            demand,
            span: pattern_span(&ctx.model.pattern(pattern).origin),
        });
    }
    Ok(children)
}

fn guaranteed_child(
    model: &Model,
    mut pattern: PatternId,
    budget: &mut Budget,
) -> Result<bool, AnalysisLimit> {
    let mut visited = HashSet::new();
    loop {
        budget.charge()?;
        if !visited.insert(pattern) {
            return Ok(false);
        }

        pattern = match &model.pattern(pattern).kind {
            PatternKind::Node(_) | PatternKind::Leaf(_) => return Ok(true),
            PatternKind::Repeat(repeat) if repeat.kind == ast::QuantifierKind::OneOrMore => {
                repeat.inner
            }
            PatternKind::Reference(definition) => model.definition(*definition),
            _ => return Ok(false),
        }
    }
}

fn emit_order_failure(
    ctx: Context<'_>,
    culprit: &ConcreteCulprit,
    order: OrderDiagnosis,
    diag: &mut Diagnostics,
) {
    let kind_name = culprit.kind_name(ctx);
    let before = order.before.demand.label(ctx);
    let after = order.after.demand.label(ctx);
    let detail = format!("the grammar places {before} before {after} in {kind_name}");
    diag.report(DiagnosticKind::UnsatisfiablePattern, order.after.span)
        .detail(detail)
        .related_to(
            order.before.span,
            format!("{before} must come before {after}"),
        )
        .related_to(culprit.span(), format!("inside {kind_name}"))
        .emit();
}

impl ChildDemand {
    fn accepts(self, child: &Child) -> bool {
        match self {
            Self::Kind(kind) => kind == child.kind,
            Self::Field(field) => Some(field) == child.field,
        }
    }

    fn matcher(self) -> ChildMatcher {
        match self {
            Self::Field(field) => ChildMatcher::new(KindConstraint::AnyNode, Some(field)),
            Self::Kind(kind) => ChildMatcher::new(KindConstraint::Exact(kind), None),
        }
    }

    fn label(self, ctx: Context<'_>) -> String {
        match self {
            Self::Field(field) => format!(
                "`{}:`",
                ctx.grammar
                    .field_name(field)
                    .expect("resolved field has a name")
            ),
            Self::Kind(kind) => render_kind(ctx, kind),
        }
    }
}

pub(super) fn report_unexplained(ctx: Context<'_>, pattern: PatternId, diag: &mut Diagnostics) {
    match Goal::from_pattern(ctx.model, pattern) {
        Some(Goal::Concrete { kind, .. }) => {
            emit_generic_failure(ctx, &ConcreteCulprit::new(ctx.model, pattern, kind), diag)
        }
        Some(Goal::Wildcard { .. }) => emit_wildcard_failure(ctx.model, pattern, diag),
        None => {
            diag.report(
                DiagnosticKind::UnsatisfiablePattern,
                pattern_span(&ctx.model.pattern(pattern).origin),
            )
            .detail("the grammar builds no tree matching this pattern")
            .emit();
        }
    }
}

fn emit_generic_failure(ctx: Context<'_>, culprit: &ConcreteCulprit, diag: &mut Diagnostics) {
    let kind_name = culprit.kind_name(ctx);
    let detail = format!("the grammar builds no {kind_name} matching this child structure");
    diag.report(DiagnosticKind::UnsatisfiablePattern, culprit.span())
        .detail(detail)
        .emit();
}

fn pattern_span(pattern: &Located<Pattern>) -> Span {
    pattern.span_of(pattern.node().text_range())
}

fn kind_span(node: &Located<NamedNodePattern>) -> Span {
    let range = node
        .node()
        .kind_token()
        .expect("validated node has a kind")
        .text_range();
    node.span_of(range)
}

fn render_kind(ctx: Context<'_>, kind: NodeKindId) -> String {
    format!("`{}`", format_kind(ctx.grammar, kind))
}

fn indefinite_article(rendered_kind: &str) -> &'static str {
    match rendered_kind
        .chars()
        .find(|c| c.is_ascii_alphabetic())
        .map(|c| c.to_ascii_lowercase())
    {
        Some('a' | 'e' | 'i' | 'o' | 'u') => "an",
        _ => "a",
    }
}
