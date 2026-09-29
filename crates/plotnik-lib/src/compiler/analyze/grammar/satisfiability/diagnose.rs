//! Explain established failures using resolved constraints and solver probes.
//! A specific message needs either a local contradiction or a probe that proves
//! its claim. If refinement runs out of work, retain the original rejection.

use std::collections::HashSet;

use crate::compiler::analyze::Located;
use crate::compiler::analyze::boundary::PendingAnchor;
use crate::compiler::diagnostics::report::{DiagnosticKind, Diagnostics, Span};
use crate::compiler::parse::ast::{self, NamedNodePattern, Pattern};
use crate::core::grammar::Grammar;
use crate::core::{NodeFieldId, NodeKindId};

use super::super::model::{Item, KindConstraint, Model, PatternId, PatternKind};
use super::AnalysisLimit;
use super::automaton::{ChildAutomaton, ChildMatcher};
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

/// Emit the impossibility diagnostic for a failed root goal.
pub(super) fn report_goal(
    ctx: Context<'_>,
    solver: &mut SatisfiabilitySolver,
    goal: Goal,
    diag: &mut Diagnostics,
    reported: &mut ReportedCulprits,
) -> Result<(), AnalysisLimit> {
    if super::local::LocalDiagnostics::report(
        ctx.grammar,
        ctx.model,
        goal.pattern(),
        diag,
        &mut reported.nodes,
    ) {
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
        Err(node) => {
            emit_wildcard_failure(&node, diag);
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

    fn from_goal(model: &Model, goal: Goal) -> Result<Self, Located<NamedNodePattern>> {
        match goal {
            Goal::Concrete { pattern, kind } => Ok(Self::new(model, pattern, kind)),
            Goal::Wildcard { pattern } => Err(model.node_origin(pattern)),
        }
    }

    fn span(&self) -> Span {
        kind_span(&self.node)
    }

    fn kind_name(&self, ctx: Context<'_>) -> String {
        render_kind(ctx, self.kind)
    }
}

/// Emit an impossible wildcard parent `(_ …)`: no kind the grammar builds takes the
/// children it constrains. A wildcard fixes no kind of its own, so there is no single
/// node to blame — the obstacle is that no production anywhere realizes this child list.
fn emit_wildcard_failure(node: &Located<NamedNodePattern>, diag: &mut Diagnostics) {
    diag.report(
        DiagnosticKind::UnsatisfiablePattern,
        node_children_span(node),
    )
    .detail("no named node in the grammar satisfies these child constraints".to_string())
    .emit();
}

/// Reject a query whose satisfiability analysis hit a resource ceiling — too many
/// automaton states (an exponentially widening expansion) or a too-deep inlined
/// reference. This is a resource limit, *not* an impossibility claim: the query may be
/// perfectly valid, just larger than the compiler will spend on it. We point at the
/// definition under analysis when the ceiling tripped.
pub(super) fn report_too_complex(body: &Located<Pattern>, diag: &mut Diagnostics) {
    let span = Span::new(body.source(), body.node().syntax().text_range());
    emit_too_complex(span, diag);
}

fn emit_too_complex(span: Span, diag: &mut Diagnostics) {
    diag.report(DiagnosticKind::QueryTooComplex, span)
        .hint(
            "simplify the pattern. Deeply nested or repeatedly referenced alternations can expand exponentially",
        )
        .emit();
}

/// A single-valued field the culprit binds more than once, with the repeat count. Such a
/// field can hold one child, so binding it twice is impossible whatever else matches —
/// the first such field in source order, named for the message.
fn repeated_single_field(
    ctx: Context<'_>,
    solver: &mut SatisfiabilitySolver,
    culprit: &ConcreteCulprit,
) -> Result<Option<(String, Vec<Span>)>, AnalysisLimit> {
    let mut fields: Vec<(NodeFieldId, Vec<Span>)> = Vec::new();
    for child in query_child_demands(ctx, culprit.pattern) {
        let ChildDemand::Field(id) = child.demand else {
            continue;
        };
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
        if solver.probe(culprit.kind, ChildAutomaton::ordered_pair(matcher, matcher))? {
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
        // The absence test covers every realization, including leaf aliases.
        if !solver.probe(culprit.kind, ChildAutomaton::without_field(field.id))? {
            diag.report(DiagnosticKind::NegatedRequiredField, span)
                .detail(name)
                .related_to(culprit.span(), format!("on {kind}"))
                .hint(format!("`-{name}` requires `{name}` to be absent, but every {kind} has one. Drop `-{name}`"))
                .emit();
            reported = true;
            continue;
        }

        let mut relaxed = solver.automaton(culprit.pattern).clone();
        relaxed.allow_field(field.id);
        if solver.probe(culprit.kind, relaxed)? {
            diag.report(DiagnosticKind::UnsatisfiablePattern, span)
                .detail(format!("`-{name}` requires `{name}` to be absent, but these {kind} child constraints require it to be present"))
                .related_to(children_span(ctx.model, culprit.pattern), "these children require the field to be present")
                .hint(format!("drop `-{name}` to allow these child patterns"))
                .emit();
            reported = true;
        }
    }
    Ok(reported)
}

/// Walk down from an unsatisfiable node to the deepest node whose children are each
/// satisfiable on their own — the point where the arrangement, not any one child, is
/// what the grammar rejects.
fn locate(
    model: &Model,
    solver: &mut SatisfiabilitySolver,
    mut goal: Goal,
) -> Result<Goal, AnalysisLimit> {
    let mut visited = HashSet::new();
    while visited.insert(goal.pattern()) {
        let mut children = Vec::new();
        for child in model
            .node(goal.pattern())
            .children
            .iter()
            .filter_map(|item| item.pattern())
        {
            collect_goals(child, model, &mut children);
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

/// Anchor locations survive compilation, including inlined references. Test the
/// proposed removal with all other constraints intact before offering that edit.
fn report_anchors(
    ctx: Context<'_>,
    solver: &mut SatisfiabilitySolver,
    culprit: &ConcreteCulprit,
    diag: &mut Diagnostics,
) -> Result<bool, AnalysisLimit> {
    let original = solver.automaton(culprit.pattern).clone();
    let mut anchors: Vec<(Span, Vec<u32>)> = Vec::new();
    let mut relaxed = original.clone();
    for state in 0..original.state_count() as u32 {
        let Some((_, span)) = original.anchor(state) else {
            continue;
        };
        if let Some((_, states)) = anchors.iter_mut().find(|(seen, _)| *seen == span) {
            states.push(state);
        } else {
            anchors.push((span, vec![state]));
        }
        relaxed.remove_anchor(span);
    }
    if anchors.is_empty() || !solver.probe(culprit.kind, relaxed)? {
        return Ok(false);
    }

    let kind = culprit.kind_name(ctx);
    let shapes = solver.child_shapes(culprit.kind);
    for (span, states) in &anchors {
        let mut removed = original.clone();
        removed.remove_anchor(*span);
        match solver.probe(culprit.kind, removed) {
            Ok(false) => continue,
            Err(AnalysisLimit) => break,
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

/// Child constraints still fail after this node's anchors have been relaxed.
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

    let shapes = solver.child_shapes(culprit.kind);
    let demands = query_child_demands(ctx, culprit.pattern);
    if let Some(shapes) = &shapes
        && report_child_count(ctx, culprit, &demands, shapes, diag)
    {
        return Ok(());
    }

    let kind = culprit.kind_name(ctx);
    let mut builder = diag
        .report(
            DiagnosticKind::UnsatisfiablePattern,
            children_span(ctx.model, culprit.pattern),
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
    diag: &mut Diagnostics,
) -> bool {
    for (index, child) in demands.iter().enumerate() {
        if matches!(child.demand, ChildDemand::Kind(kind) if ctx.grammar.is_extra(kind)) {
            continue;
        }
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
        return true;
    }
    false
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
                    let name = ctx
                        .grammar
                        .node_kind(child.kind)
                        .expect("retained kind has a name");
                    let name = if ctx.grammar.is_anonymous_node(child.kind) {
                        format!("{name:?}")
                    } else {
                        name.to_string()
                    };
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

fn children_span(model: &Model, pattern: PatternId) -> Span {
    let origin = model.node_origin(pattern);
    node_children_span(&origin)
}

fn node_children_span(origin: &Located<NamedNodePattern>) -> Span {
    let mut children = origin.node().children();
    let Some(first) = children.next() else {
        return kind_span(origin);
    };
    let last = children.last().unwrap_or_else(|| first.clone());
    origin.span_of(first.text_range().cover(last.text_range()))
}

/// A nested pattern can work on some realization of its public kind and still
/// fail at this position (notably when aliases share a kind). Keep its kind and
/// field, and test whether removing only its child constraints permits a match.
fn report_nested_constraints(
    ctx: Context<'_>,
    solver: &mut SatisfiabilitySolver,
    culprit: &ConcreteCulprit,
    diag: &mut Diagnostics,
) -> Result<bool, AnalysisLimit> {
    let original = solver.automaton(culprit.pattern).clone();
    let mut seen = HashSet::new();
    for nested in original.nested_patterns() {
        if !seen.insert(nested) {
            continue;
        }
        let mut relaxed = original.clone();
        relaxed.allow_nested(nested);
        if !solver.probe(culprit.kind, relaxed)? {
            continue;
        }

        let parent = culprit.kind_name(ctx);
        let resolved = ctx.model.node(nested);
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
        let span = childless_anchor.unwrap_or_else(|| children_span(ctx.model, nested));
        let detail = if empty {
            format!(
                "this pattern requires a childless node, but this position in {parent} needs a node with children"
            )
        } else {
            format!("these nested child constraints cannot be satisfied inside {parent}")
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
    let query = query_child_demands(ctx, culprit.pattern);
    for (index, before) in query.iter().enumerate() {
        for after in &query[index + 1..] {
            if before.demand == after.demand {
                continue;
            }

            let requested =
                ChildAutomaton::ordered_pair(before.demand.matcher(), after.demand.matcher());
            if solver.probe(culprit.kind, requested)? {
                continue;
            }

            let reversed =
                ChildAutomaton::ordered_pair(after.demand.matcher(), before.demand.matcher());
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

fn query_child_demands(ctx: Context<'_>, node: PatternId) -> Vec<QueryChild> {
    let mut children = Vec::new();
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
        let demand = match &ctx.model.pattern(pattern).kind {
            PatternKind::Field(field, inner)
                if guaranteed_child(ctx.model, *inner, &mut HashSet::new()) =>
            {
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
    children
}

// A field wrapped around a nullable pattern need not consume a field at all.
// Sequences discard an inherited field in the child automaton, so they do not
// establish a demand for that outer field either.
fn guaranteed_child(model: &Model, pattern: PatternId, visited: &mut HashSet<PatternId>) -> bool {
    if !visited.insert(pattern) {
        return false;
    }
    match &model.pattern(pattern).kind {
        PatternKind::Node(_) | PatternKind::Leaf(_) => true,
        PatternKind::Repeat(repeat) if repeat.kind == ast::QuantifierKind::OneOrMore => {
            guaranteed_child(model, repeat.inner, visited)
        }
        PatternKind::Reference(definition) => {
            guaranteed_child(model, model.definition(*definition), visited)
        }
        _ => false,
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
        Some(Goal::Wildcard { .. }) => emit_wildcard_failure(&ctx.model.node_origin(pattern), diag),
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

/// The primary solve proved the node impossible, but diagnostic refinement ran out
/// before it could distinguish anchors from child order. Keep the verdict precise
/// without pretending to know the specific obstacle.
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

/// Render one kind: a named kind in backticks, an anonymous one as a quoted literal.
fn render_kind(ctx: Context<'_>, kind: NodeKindId) -> String {
    let name = ctx
        .grammar
        .node_kind(kind)
        .expect("resolved kind has a name");
    if ctx.grammar.is_anonymous_node(kind) {
        return format!("`\"{name}\"`");
    }

    format!("`{name}`")
}

/// "a" or "an" for a rendered kind like `` `array` `` — chosen on the first letter
/// inside the quoting, so a vowel-initial kind (`an array`) reads naturally.
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
