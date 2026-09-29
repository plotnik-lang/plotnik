//! Diagnostics for an impossible pattern.
//!
//! A failed root goal only says the *whole* pattern can never match — rarely where
//! the user should look. This module pushes the blame to the deepest node whose own
//! children are each satisfiable in isolation, yet which the grammar can build with
//! no node of that kind: the smallest edit site, the "deepest failing position"
//! parsers point at.
//!
//! It then names the obstacle. If relaxing the node's anchors would let it match
//! (proven by re-solving with only that node's gaps widened), the anchors are to blame, and the
//! message contrasts the positions they demand with what the grammar actually places
//! first or last. Otherwise the children's kinds or order are the obstacle, and the
//! message lists what a node of that kind does allow. Each alternative produces its own
//! shape of message — a leading anchor reads differently from a trailing one, which
//! reads differently from a wrong arrangement — so the variety mirrors the cause.

use std::collections::HashSet;

use crate::compiler::analyze::Located;
use crate::compiler::analyze::anchors::{AnchorSemantics, GapClass};
use crate::compiler::analyze::shape::PatternFacts;
use crate::compiler::diagnostics::report::{DiagnosticKind, Diagnostics, Span};
use crate::compiler::diagnostics::source::{SourceId, SourceMap};
use crate::compiler::parse::ast::{self, NamedNodePattern, Pattern, SeqItem};
use crate::core::grammar::Grammar;
use crate::core::{NodeFieldId, NodeKindId};

use super::super::model::{KindConstraint, Model, PatternId, PatternKind};
use super::AnalysisLimit;
use super::engine::SatisfiabilitySolver;
use super::{Goal, Participation, collect_goals};

#[derive(Clone, Copy)]
pub(in crate::compiler::analyze::grammar) struct Context<'a> {
    pub(in crate::compiler::analyze::grammar) grammar: &'a Grammar,
    pub(in crate::compiler::analyze::grammar) model: &'a Model,
    pub(in crate::compiler::analyze::grammar) pattern_facts: &'a PatternFacts,
    pub(in crate::compiler::analyze::grammar) source_map: &'a SourceMap,
}

impl<'a> Context<'a> {
    fn content(self, source: SourceId) -> &'a str {
        self.source_map.content(source)
    }
}

pub(super) enum ReportOutcome {
    Emitted,
    Stop,
}

impl ReportOutcome {
    pub(super) fn should_stop(&self) -> bool {
        matches!(self, Self::Stop)
    }
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
    diagnostic_budget: &mut u64,
    reported: &mut ReportedCulprits,
) -> Result<ReportOutcome, AnalysisLimit> {
    let mut visited = HashSet::new();
    let culprit = locate(ctx.model, solver, goal, &mut visited)?;
    if !reported.insert(&culprit) {
        return Ok(ReportOutcome::Emitted);
    }
    Ok(report_culprit(
        ctx,
        solver,
        culprit,
        diag,
        diagnostic_budget,
    ))
}

fn report_culprit(
    ctx: Context<'_>,
    solver: &mut SatisfiabilitySolver,
    culprit: Goal,
    diag: &mut Diagnostics,
    diagnostic_budget: &mut u64,
) -> ReportOutcome {
    let culprit = match ConcreteCulprit::from_goal(ctx.model, culprit) {
        Ok(culprit) => culprit,
        Err(node) => {
            emit_wildcard_failure(&node, diag);
            return ReportOutcome::Emitted;
        }
    };

    if let Some(conflict) = negated_matched_field(ctx, &culprit) {
        emit_negated_field_conflict(ctx, &culprit, conflict, diag);
        return ReportOutcome::Emitted;
    }

    if let Some(field) = negated_required_field(ctx, &culprit) {
        emit_negated_required_field(ctx, &culprit, field, diag);
        return ReportOutcome::Emitted;
    }

    // A single-valued field bound more than once is the whole obstacle on its own —
    // a sharper thing to say than a vague "combination the grammar never produces".
    if let Some((field, count)) = repeated_single_field(ctx, &culprit) {
        emit_repeated_field_failure(ctx, &culprit, &field, count, diag);
        return ReportOutcome::Emitted;
    }

    let anchor_probe = if has_anchor(culprit.node.node()) {
        probe_anchors(solver, culprit.pattern, culprit.kind, diagnostic_budget)
    } else {
        AnchorProbe::DoesNotMatch
    };

    match anchor_probe {
        AnchorProbe::Matches => emit_anchor_failure(ctx, solver, &culprit, diag),
        AnchorProbe::DoesNotMatch => emit_arrangement_failure(ctx, &culprit, diag),
        AnchorProbe::Inconclusive => {
            emit_generic_failure(ctx, &culprit, diag);
            return ReportOutcome::Stop;
        }
    }
    ReportOutcome::Emitted
}

struct ConcreteCulprit {
    pattern: PatternId,
    node: Located<NamedNodePattern>,
    kind: NodeKindId,
}

impl ConcreteCulprit {
    fn from_goal(model: &Model, goal: Goal) -> Result<Self, Located<NamedNodePattern>> {
        match goal {
            Goal::Concrete { pattern, kind } => Ok(Self {
                pattern,
                node: model.node_origin(pattern),
                kind,
            }),
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
    diag.report(DiagnosticKind::UnsatisfiablePattern, kind_span(node))
        .detail("no node the grammar builds takes these children".to_string())
        .hint(
            "`(_)` matches any named node, but no named node kind admits this combination of \
             children in this order",
        )
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
fn repeated_single_field(ctx: Context<'_>, culprit: &ConcreteCulprit) -> Option<(String, usize)> {
    let grammar = ctx.grammar;
    // First-seen order, so the message blames the field the reader meets first.
    let mut counts: Vec<(NodeFieldId, usize)> = Vec::new();
    for child in culprit.node.node().children() {
        let Pattern::FieldPattern(field) = child else {
            continue;
        };
        let Some(name) = field.name() else { continue };
        let Some(id) = grammar.resolve_field(name.text()) else {
            continue;
        };
        match counts.iter_mut().find(|(seen, _)| *seen == id) {
            Some((_, count)) => *count += 1,
            None => counts.push((id, 1)),
        }
    }

    counts.into_iter().find_map(|(id, count)| {
        let single = grammar
            .field_cardinality(culprit.kind, id)
            .is_some_and(|cardinality| !cardinality.is_multiple());
        if count < 2 || !single {
            return None;
        }
        Some((grammar.field_name(id)?.to_string(), count))
    })
}

fn emit_repeated_field_failure(
    ctx: Context<'_>,
    culprit: &ConcreteCulprit,
    field: &str,
    count: usize,
    diag: &mut Diagnostics,
) {
    let kind_name = culprit.kind_name(ctx);
    let article = indefinite_article(&kind_name);
    let detail = format!(
        "{article} {kind_name} has one `{field}`, but this pattern binds `{field}` {count} times"
    );
    diag.report(DiagnosticKind::UnsatisfiablePattern, culprit.span())
        .detail(detail)
        .hint(format!("keep a single `{field}:` child"))
        .emit();
}

#[derive(Clone)]
struct NegatedFieldUse {
    id: NodeFieldId,
    name: String,
    span: Span,
}

struct NegatedFieldConflict {
    field: NegatedFieldUse,
    matched_span: Span,
}

fn negated_fields(ctx: Context<'_>, culprit: &ConcreteCulprit) -> Vec<NegatedFieldUse> {
    culprit
        .node
        .node()
        .syntax()
        .children()
        .filter_map(ast::NegatedField::cast)
        .filter_map(|negated| {
            let name = negated
                .name()
                .expect("admitted negated field must have a field name");
            Some(NegatedFieldUse {
                id: ctx.grammar.resolve_field(name.text())?,
                name: name.text().to_string(),
                span: culprit.node.span_of(name.text_range()),
            })
        })
        .collect()
}

fn negated_matched_field(
    ctx: Context<'_>,
    culprit: &ConcreteCulprit,
) -> Option<NegatedFieldConflict> {
    for field in negated_fields(ctx, culprit) {
        let matched_span = ctx
            .model
            .node(culprit.pattern)
            .children
            .iter()
            .find_map(|child| {
                pattern_matches_field(ctx, culprit.kind, child.pattern, field.id)
                    .then(|| pattern_span(&ctx.model.pattern(child.pattern).origin))
            });
        if let Some(matched_span) = matched_span {
            return Some(NegatedFieldConflict {
                field,
                matched_span,
            });
        }
    }
    None
}

fn negated_required_field(ctx: Context<'_>, culprit: &ConcreteCulprit) -> Option<NegatedFieldUse> {
    negated_fields(ctx, culprit).into_iter().find(|field| {
        ctx.grammar
            .field_cardinality(culprit.kind, field.id)
            .is_some_and(|cardinality| cardinality.is_required())
    })
}

fn pattern_matches_field(
    ctx: Context<'_>,
    parent: NodeKindId,
    pattern: PatternId,
    field: NodeFieldId,
) -> bool {
    match &ctx.model.pattern(pattern).kind {
        PatternKind::Field(id, _) => *id == field,
        PatternKind::Repeat(ast::QuantifierKind::OneOrMore, inner) => {
            pattern_matches_field(ctx, parent, *inner, field)
        }
        PatternKind::Node(node) => match node.kind {
            KindConstraint::Exact(kind) => field_value_admits(ctx, parent, field, kind),
            _ => false,
        },
        PatternKind::Reference(definition) => {
            match Goal::from_pattern(ctx.model, ctx.model.definition(*definition)) {
                Some(Goal::Concrete { kind, .. }) => field_value_admits(ctx, parent, field, kind),
                _ => false,
            }
        }
        _ => false,
    }
}

fn field_value_admits(
    ctx: Context<'_>,
    parent: NodeKindId,
    field: NodeFieldId,
    value: NodeKindId,
) -> bool {
    let grammar = ctx.grammar;
    grammar
        .valid_field_types(parent, field)
        .iter()
        .any(|&declared| declared == value || grammar.collect_subtypes(declared).contains(&value))
}

fn emit_negated_field_conflict(
    ctx: Context<'_>,
    culprit: &ConcreteCulprit,
    conflict: NegatedFieldConflict,
    diag: &mut Diagnostics,
) {
    let kind_name = culprit.kind_name(ctx);
    let field = conflict.field.name;
    let detail = format!(
        "`-{field}` requires `{field}` to be absent, but this {kind_name} pattern also matches `{field}`"
    );
    diag.report(DiagnosticKind::UnsatisfiablePattern, conflict.field.span)
        .detail(detail)
        .related_to(conflict.matched_span, format!("matches `{field}` here"))
        .hint(format!("drop `-{field}` or remove the `{field}` child"))
        .emit();
}

fn emit_negated_required_field(
    ctx: Context<'_>,
    culprit: &ConcreteCulprit,
    field: NegatedFieldUse,
    diag: &mut Diagnostics,
) {
    let kind_name = culprit.kind_name(ctx);
    diag.report(DiagnosticKind::NegatedRequiredField, field.span)
        .detail(field.name.clone())
        .related_to(culprit.span(), format!("on {kind_name}"))
        .hint(format!(
            "`-{0}` requires `{0}` to be absent, but every {1} has one. Drop `-{0}`",
            field.name, kind_name
        ))
        .emit();
}

/// Walk down from an unsatisfiable node to the deepest node whose children are each
/// satisfiable on their own — the point where the arrangement, not any one child, is
/// what the grammar rejects.
fn locate(
    model: &Model,
    solver: &mut SatisfiabilitySolver,
    goal: Goal,
    visited: &mut HashSet<PatternId>,
) -> Result<Goal, AnalysisLimit> {
    if !visited.insert(goal.pattern()) {
        // A recursive definition folded back onto a node already being blamed.
        return Ok(goal);
    }

    let mut children = Vec::new();
    for child in &model.node(goal.pattern()).children {
        collect_goals(child.pattern, Participation::Required, model, &mut children);
    }

    for child in children {
        if child.is_impossible(solver)? {
            return locate(model, solver, child, visited);
        }
    }

    Ok(goal)
}

enum AnchorProbe {
    Matches,
    DoesNotMatch,
    Inconclusive,
}

/// Relax only the culprit's gaps. Nested constraints must still hold before we
/// can blame this node's anchors. Each probe has different premises, so it needs
/// fresh solve results. Grammar facts and the total diagnostic budget are shared.
fn probe_anchors(
    primary: &SatisfiabilitySolver,
    pattern: PatternId,
    kind: NodeKindId,
    budget: &mut u64,
) -> AnchorProbe {
    if *budget == 0 {
        return AnchorProbe::Inconclusive;
    }

    let mut probe = primary.relaxing_anchors(pattern, *budget);
    let result = probe.satisfiable(pattern, kind);
    *budget = probe.remaining_budget();
    match result {
        Ok(true) => AnchorProbe::Matches,
        Ok(false) => AnchorProbe::DoesNotMatch,
        Err(AnalysisLimit) => AnchorProbe::Inconclusive,
    }
}

fn emit_anchor_failure(
    ctx: Context<'_>,
    solver: &SatisfiabilitySolver,
    culprit: &ConcreteCulprit,
    diag: &mut Diagnostics,
) {
    let node_pattern = culprit.node.node();
    let kind_name = culprit.kind_name(ctx);
    let span = culprit.span();
    let exact = has_exact_anchor(node_pattern);

    // A leading anchor pins the first child; a trailing one the last. The two read
    // differently, so each gets its own message naming the boundary the grammar fixes.
    let Some(boundary) = Boundary::of(node_pattern) else {
        return emit_interior_anchor_failure(ctx, culprit, exact, diag);
    };
    let boundary_name = boundary.name();
    let boundary_verb = boundary.verb();
    let allowed = boundary.child_kinds(solver, culprit.kind);
    let wanted = boundary.pattern_label(culprit.pattern, ctx);

    let demand = if exact {
        "with the exact anchor (`.!`)"
    } else {
        "after the soft anchor (`.`)"
    };
    let detail = match &wanted {
        Some(want) => format!(
            "{demand}, the {boundary_name} child of {kind_name} must be {want}, \
             but {kind_name} {} {}",
            boundary_verb,
            render_kind_list(ctx, &allowed, "other kinds"),
        ),
        None => format!(
            "{demand}, this child cannot appear {boundary_name} in {kind_name} because \
             {} {kind_name} {} {}",
            indefinite_article(&kind_name),
            boundary_verb,
            render_kind_list(ctx, &allowed, "no fixed kind"),
        ),
    };

    let mut builder = diag
        .report(DiagnosticKind::UnsatisfiablePattern, span)
        .detail(detail);
    builder = builder.hint(anchor_fix_hint(&culprit.node, ctx, exact));
    builder.emit();
}

/// An anchor between two children — neither end is pinned, so explain the adjacency.
fn emit_interior_anchor_failure(
    ctx: Context<'_>,
    culprit: &ConcreteCulprit,
    exact: bool,
    diag: &mut Diagnostics,
) {
    let kind_name = culprit.kind_name(ctx);
    let detail = format!("no {kind_name} places these children in this adjacency");
    diag.report(DiagnosticKind::UnsatisfiablePattern, culprit.span())
        .detail(detail)
        .hint(anchor_fix_hint(&culprit.node, ctx, exact))
        .emit();
}

/// The child kinds or their order are the obstacle (the anchors, if any, are not).
fn emit_arrangement_failure(ctx: Context<'_>, culprit: &ConcreteCulprit, diag: &mut Diagnostics) {
    if let Some(order) = order_diagnosis(ctx, culprit) {
        emit_order_failure(ctx, culprit, order, diag);
        return;
    }

    let kind_name = culprit.kind_name(ctx);
    let detail = format!("the grammar builds no {kind_name} with these children");
    let mut builder = diag
        .report(DiagnosticKind::UnsatisfiablePattern, culprit.span())
        .detail(detail);
    if let Some(allows) = describe_allowed_children(ctx, culprit.kind, &kind_name) {
        builder = builder.hint(allows);
    }
    builder.emit();
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

fn order_diagnosis(ctx: Context<'_>, culprit: &ConcreteCulprit) -> Option<OrderDiagnosis> {
    let query = query_child_demands(ctx, culprit.pattern);
    if query.len() < 2 {
        return None;
    }

    let mut diagnosis = None;
    for production in grammar_child_orders(ctx, culprit.kind) {
        let positions: Option<Vec<_>> = query
            .iter()
            .map(|child| production.iter().position(|demand| demand == &child.demand))
            .collect();
        let Some(positions) = positions else {
            continue;
        };
        let mut inverted = None;
        for i in 0..query.len() {
            for j in i + 1..query.len() {
                if positions[i] > positions[j] {
                    inverted = Some((i, j));
                    break;
                }
            }
            if inverted.is_some() {
                break;
            }
        }
        let (i, j) = inverted?;
        if diagnosis.is_none() {
            diagnosis = Some(OrderDiagnosis {
                before: query[j].clone(),
                after: query[i].clone(),
            });
        }
    }
    diagnosis
}

fn query_child_demands(ctx: Context<'_>, node: PatternId) -> Vec<QueryChild> {
    ctx.model
        .node(node)
        .children
        .iter()
        .filter_map(|child| {
            query_demand(ctx.model, child.pattern).map(|demand| QueryChild {
                demand,
                span: pattern_span(&ctx.model.pattern(child.pattern).origin),
            })
        })
        .collect()
}

fn query_demand(model: &Model, pattern: PatternId) -> Option<ChildDemand> {
    match &model.pattern(pattern).kind {
        PatternKind::Field(field, _) => Some(ChildDemand::Field(*field)),
        PatternKind::Node(node) => match node.kind {
            KindConstraint::Exact(kind) => Some(ChildDemand::Kind(kind)),
            _ => None,
        },
        PatternKind::Leaf(KindConstraint::Exact(kind)) => Some(ChildDemand::Kind(*kind)),
        PatternKind::Reference(definition) => {
            match Goal::from_pattern(model, model.definition(*definition)) {
                Some(Goal::Concrete { kind, .. }) => Some(ChildDemand::Kind(kind)),
                _ => None,
            }
        }
        PatternKind::Repeat(_, inner) => query_demand(model, *inner),
        _ => None,
    }
}

fn grammar_child_orders(ctx: Context<'_>, kind: NodeKindId) -> Vec<Vec<ChildDemand>> {
    let realizers_by_kind = ctx.grammar.structure().surface_realizers_by_kind();
    let Some(realizers) = realizers_by_kind.get(&kind) else {
        return Vec::new();
    };

    let mut orders = Vec::new();
    for realizer in realizers {
        let Some(body) = realizer.body else {
            continue;
        };
        let Some(variable) = ctx.grammar.structure().variable(body) else {
            continue;
        };
        for production in &variable.productions {
            let order: Vec<_> = production
                .iter()
                .filter_map(|step| {
                    step.field
                        .map(ChildDemand::Field)
                        .or_else(|| step.target.visible_kind(ctx.grammar).map(ChildDemand::Kind))
                })
                .collect();
            if !order.is_empty() {
                orders.push(order);
            }
        }
    }
    orders
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
        .hint(format!("write {before} before {after}"))
        .emit();
}

impl ChildDemand {
    fn label(self, ctx: Context<'_>) -> String {
        match self {
            Self::Field(field) => match ctx.grammar.field_name(field) {
                Some(name) => format!("`{name}:`"),
                None => "`<unknown field>:`".to_string(),
            },
            Self::Kind(kind) => render_kind(ctx, kind),
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
        .hint("adjust the children, anchors, or order to match a shape this grammar produces")
        .emit();
}

/// A help line listing what a node of `kind` does allow: its named child kinds and
/// fields, the same vocabulary the structural pass uses.
fn describe_allowed_children(
    ctx: Context<'_>,
    kind: NodeKindId,
    kind_name: &str,
) -> Option<String> {
    let grammar = ctx.grammar;
    let children: Vec<&str> = grammar
        .valid_child_types(kind)
        .iter()
        .filter_map(|&id| grammar.node_kind(id))
        .collect();
    let fields = grammar.fields_for_node_kind(kind);

    let mut parts = Vec::new();
    if !children.is_empty() {
        parts.push(format!("children {}", name_list(&children, 8)));
    }
    if !fields.is_empty() {
        parts.push(format!("fields {}", name_list(&fields, 8)));
    }
    if parts.is_empty() {
        return Some(format!("{kind_name} takes no named children"));
    }
    Some(format!("{kind_name} allows {}", parts.join(", ")))
}

/// The fix hint: for an exact anchor, the soft form often matches; for a soft anchor,
/// dropping it does. Shows the rewritten node where it can be derived by swapping the
/// anchor token in the node's own source.
fn anchor_fix_hint(node: &Located<NamedNodePattern>, ctx: Context<'_>, exact: bool) -> String {
    if exact {
        match relaxed_anchor_text(node, ctx) {
            Some(soft) => format!(
                "`.!` allows no syntax-tree node in its gap, including anonymous \
                 tokens and comments. The soft anchor `.` skips those: `{soft}`"
            ),
            None => "`.!` allows no syntax-tree node in its gap, including anonymous \
                 tokens and comments. Use the soft anchor `.`, which skips them"
                .to_string(),
        }
    } else {
        if soft_anchor_skips_extras_only(node, ctx) {
            return "here `.` skips extras only because one side can match an anonymous token. \
                    Use `(_)` for a named node, or drop the anchor to match these children \
                    in any positions"
                .to_string();
        }
        "`.` skips anonymous tokens and extras but never another named node. \
         Drop the anchor to match these children in any positions"
            .to_string()
    }
}

/// The node's own source with `.!` rewritten to `.` — a concrete suggestion.
fn relaxed_anchor_text(node: &Located<NamedNodePattern>, ctx: Context<'_>) -> Option<String> {
    let range = node.node().text_range();
    let text = ctx.content(node.source());
    let slice = text.get(usize::from(range.start())..usize::from(range.end()))?;
    slice.contains(".!").then(|| slice.replace(".!", "."))
}

fn soft_anchor_skips_extras_only(node: &Located<NamedNodePattern>, ctx: Context<'_>) -> bool {
    let items: Vec<_> = node.node().items().collect();
    let semantics = AnchorSemantics::new(ctx.pattern_facts);
    semantics
        .compute_nav_modes(&items, true)
        .into_iter()
        .any(|(_, nav)| nav.and_then(GapClass::from_nav) == Some(GapClass::ExtrasOnly))
        || semantics
            .check_trailing_anchor(&items)
            .1
            .and_then(GapClass::from_nav)
            == Some(GapClass::ExtrasOnly)
}

fn has_anchor(node: &NamedNodePattern) -> bool {
    node.items().any(|item| matches!(item, SeqItem::Anchor(_)))
}

#[derive(Clone, Copy)]
enum Boundary {
    First,
    Last,
}

impl Boundary {
    fn of(node: &NamedNodePattern) -> Option<Self> {
        if matches!(node.items().next(), Some(SeqItem::Anchor(_))) {
            Some(Self::First)
        } else if matches!(node.items().last(), Some(SeqItem::Anchor(_))) {
            Some(Self::Last)
        } else {
            None
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::First => "first",
            Self::Last => "last",
        }
    }

    fn verb(self) -> &'static str {
        match self {
            Self::First => "begins with",
            Self::Last => "ends with",
        }
    }

    fn child_kinds(self, solver: &SatisfiabilitySolver, kind: NodeKindId) -> Vec<NodeKindId> {
        match self {
            Self::First => solver.first_child_kinds(kind),
            Self::Last => solver.last_child_kinds(kind),
        }
    }

    fn pattern_label(self, node: PatternId, ctx: Context<'_>) -> Option<String> {
        let children = &ctx.model.node(node).children;
        let child = match self {
            Self::First => children.first()?,
            Self::Last => children.last()?,
        };
        pattern_label(child.pattern, ctx)
    }
}

/// Whether any anchor in the node is exact — exact anchoring is the tighter constraint,
/// so its explanation governs when both kinds are present.
fn has_exact_anchor(node: &NamedNodePattern) -> bool {
    node.items()
        .any(|item| matches!(item, SeqItem::Anchor(a) if a.is_exact()))
}

/// A short label for a child pattern, leaving compound forms unnamed.
fn pattern_label(pattern: PatternId, ctx: Context<'_>) -> Option<String> {
    let pattern = ctx.model.pattern(pattern);
    match &pattern.kind {
        PatternKind::Node(node) => match node.kind {
            KindConstraint::AnyNamed => Some("any named node".to_string()),
            KindConstraint::Exact(kind) => Some(render_kind(ctx, kind)),
            _ => None,
        },
        PatternKind::Leaf(KindConstraint::Exact(_)) => {
            let syntax = pattern.syntax();
            let Pattern::AnonymousNodePattern(node) = syntax.node() else {
                unreachable!("exact leaf originates at an anonymous node");
            };
            Some(format!("`\"{}\"`", node.value()?.text()))
        }
        PatternKind::Leaf(KindConstraint::AnyNode) => Some("any node".to_string()),
        _ => None,
    }
}

fn pattern_span(pattern: &Located<Pattern>) -> Span {
    pattern.span_of(pattern.node().text_range())
}

/// The span of a node pattern's kind token, falling back to the whole node.
fn kind_span(node: &Located<NamedNodePattern>) -> Span {
    let range = node
        .node()
        .kind_token()
        .map_or_else(|| node.node().text_range(), |token| token.text_range());
    node.span_of(range)
}

/// Render one kind: a named kind in backticks, an anonymous one as a quoted literal.
fn render_kind(ctx: Context<'_>, kind: NodeKindId) -> String {
    match ctx.grammar.node_kind(kind) {
        Some(name) if ctx.grammar.is_anonymous_node(kind) => format!("`\"{name}\"`"),
        Some(name) => format!("`{name}`"),
        None => "an unknown kind".to_string(),
    }
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

/// Render a set of kinds as "`a`, `b` or `c`", eliding past `max`, with `empty` when
/// the set is empty (a node whose boundary the analysis could not pin).
fn render_kind_list(ctx: Context<'_>, kinds: &[NodeKindId], empty: &str) -> String {
    if kinds.is_empty() {
        return empty.to_string();
    }
    let rendered: Vec<String> = kinds.iter().map(|&id| render_kind(ctx, id)).collect();
    join_or(&rendered, 6)
}

/// Render a list of plain names in backticks, comma-joined, eliding past `max`.
fn name_list(names: &[&str], max: usize) -> String {
    let quoted: Vec<String> = names.iter().map(|name| format!("`{name}`")).collect();
    join_or(&quoted, max)
}

/// Join already-rendered items as "a, b or c", eliding the tail past `max` as "…".
fn join_or(items: &[String], max: usize) -> String {
    if items.len() > max {
        return format!("{}, …", items[..max].join(", "));
    }
    match items {
        [] => String::new(),
        [only] => only.clone(),
        [head @ .., last] => format!("{} or {last}", head.join(", ")),
    }
}
