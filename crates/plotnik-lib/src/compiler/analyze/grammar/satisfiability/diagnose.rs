//! Diagnostics for an impossible pattern.
//!
//! A failed root goal only says the *whole* pattern can never match — rarely where
//! the user should look. This module pushes the blame to the deepest node whose own
//! children are each satisfiable in isolation, yet which the grammar can build with
//! no node of that kind: the smallest edit site, the "deepest failing position"
//! parsers point at.
//!
//! It then names the obstacle. If relaxing the node's anchors would let it match
//! (proven by re-solving with every gap widened), the anchors are to blame, and the
//! message contrasts the adjacency they demand with what the grammar actually places
//! first or last. Otherwise the children's kinds or order are the obstacle, and the
//! message lists what a node of that kind does allow. Each branch produces its own
//! shape of message — a leading anchor reads differently from a trailing one, which
//! reads differently from a wrong arrangement — so the variety mirrors the cause.

use std::collections::HashSet;

use crate::compiler::analyze::Located;
use crate::compiler::diagnostics::report::{DiagnosticKind, Diagnostics, Span};
use crate::compiler::parse::ast::{NodePattern, Pattern, SeqItem};
use crate::compiler::parse::cst::SyntaxNode;
use crate::core::{NodeFieldId, NodeKindId};

use super::automaton::AutomatonContext;
use super::engine::SatisfiabilitySolver;
use super::{Goal, Participation, collect_goals, root_kind};

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
    nodes: HashSet<SyntaxNode>,
}

impl ReportedCulprits {
    fn insert(&mut self, culprit: &Goal) -> bool {
        self.nodes.insert(culprit.node().node().syntax().clone())
    }
}

/// Emit the impossibility diagnostic for a failed root goal.
pub(super) fn report_goal(
    solver: &mut SatisfiabilitySolver,
    goal: Goal,
    diag: &mut Diagnostics,
    anchor_probes: &mut AnchorProbes<'_>,
    reported: &mut ReportedCulprits,
) -> ReportOutcome {
    let mut visited = HashSet::new();
    let culprit = locate(solver, goal, &mut visited);
    if !reported.insert(&culprit) {
        return ReportOutcome::Emitted;
    }
    report_culprit(solver, culprit, diag, anchor_probes)
}

fn report_culprit(
    solver: &mut SatisfiabilitySolver,
    culprit: Goal,
    diag: &mut Diagnostics,
    anchor_probes: &mut AnchorProbes<'_>,
) -> ReportOutcome {
    if solver.is_too_complex() {
        report_node_too_complex(culprit.node(), diag);
        return ReportOutcome::Stop;
    }

    let ctx = solver.context();

    let culprit = match ConcreteCulprit::from_goal(culprit) {
        Ok(culprit) => culprit,
        Err(node) => {
            emit_wildcard_failure(&node, diag);
            return ReportOutcome::Emitted;
        }
    };

    // A single-valued field bound more than once is the whole obstacle on its own —
    // a sharper thing to say than a vague "combination the grammar never produces".
    if let Some((field, count)) = repeated_single_field(ctx, &culprit) {
        emit_repeated_field_failure(ctx, &culprit, &field, count, diag);
        return ReportOutcome::Emitted;
    }

    let anchor_probe = if has_anchor(culprit.node.node()) {
        anchor_probes.relax(&culprit.node, culprit.kind)
    } else {
        AnchorProbe::DoesNotMatch
    };

    match anchor_probe {
        AnchorProbe::Matches => emit_anchor_failure(solver, &culprit, diag),
        AnchorProbe::DoesNotMatch => emit_arrangement_failure(ctx, &culprit, diag),
        AnchorProbe::Inconclusive => {
            emit_generic_failure(ctx, &culprit, diag);
            return ReportOutcome::Stop;
        }
    }
    ReportOutcome::Emitted
}

struct ConcreteCulprit {
    node: Located<NodePattern>,
    kind: NodeKindId,
}

impl ConcreteCulprit {
    fn from_goal(goal: Goal) -> Result<Self, Located<NodePattern>> {
        match goal {
            Goal::Concrete { node, kind } => Ok(Self { node, kind }),
            Goal::Wildcard { node } => Err(node),
        }
    }

    fn span(&self) -> Span {
        kind_span(&self.node)
    }

    fn kind_name(&self, ctx: AutomatonContext<'_>) -> String {
        render_kind(ctx, self.kind)
    }
}

/// Emit an impossible wildcard parent `(_ …)`: no kind the grammar builds takes the
/// children it constrains. A wildcard fixes no kind of its own, so there is no single
/// node to blame — the obstacle is that no production anywhere realizes this child list.
fn emit_wildcard_failure(node: &Located<NodePattern>, diag: &mut Diagnostics) {
    diag.report(DiagnosticKind::UnsatisfiablePattern, kind_span(node))
        .detail("no node the grammar builds takes these children".to_string())
        .hint(
            "`(_)` and `_` match any node, but no node kind admits this combination of \
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

fn report_node_too_complex(node: &Located<NodePattern>, diag: &mut Diagnostics) {
    let span = Span::new(node.source(), node.node().syntax().text_range());
    emit_too_complex(span, diag);
}

fn emit_too_complex(span: Span, diag: &mut Diagnostics) {
    diag.report(DiagnosticKind::QueryTooComplex, span)
        .hint(
            "simplify the pattern — deeply nested or repeatedly-referenced alternations \
             can expand exponentially",
        )
        .emit();
}

/// A single-valued field the culprit binds more than once, with the repeat count. Such a
/// field can hold one child, so binding it twice is impossible whatever else matches —
/// the first such field in source order, named for the message.
fn repeated_single_field(
    ctx: AutomatonContext<'_>,
    culprit: &ConcreteCulprit,
) -> Option<(String, usize)> {
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
    ctx: AutomatonContext<'_>,
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

/// Walk down from an unsatisfiable node to the deepest node whose children are each
/// satisfiable on their own — the point where the arrangement, not any one child, is
/// what the grammar rejects.
fn locate(
    solver: &mut SatisfiabilitySolver,
    goal: Goal,
    visited: &mut HashSet<SyntaxNode>,
) -> Goal {
    if !visited.insert(goal.node().node().syntax().clone()) {
        // A recursive definition folded back onto a node already being blamed.
        return goal;
    }

    let ctx = solver.context();
    let mut children = Vec::new();
    for child in goal.node().node().children() {
        collect_goals(
            &goal.node().wrap(child),
            Participation::Required,
            ctx,
            &mut children,
        );
    }

    for child in children {
        if child.is_impossible(solver) {
            return locate(solver, child, visited);
        }
    }

    goal
}

enum AnchorProbe {
    Matches,
    DoesNotMatch,
    Inconclusive,
}

/// Re-solves anchored culprits with every gap widened to "any node may intervene".
/// It owns one relaxed solver for the whole reporting pass, so automata and fixed-point
/// memos survive across diagnostics. Exhausting this diagnostic solver never changes the
/// primary proof; it only makes later explanations less specific.
pub(super) struct AnchorProbes<'a> {
    solver: SatisfiabilitySolver<'a>,
}

impl<'a> AnchorProbes<'a> {
    pub(super) fn new(primary: &SatisfiabilitySolver<'a>, step_budget: u64) -> Self {
        Self {
            solver: primary.relaxing_anchors(step_budget),
        }
    }

    fn relax(&mut self, node: &Located<NodePattern>, kind: NodeKindId) -> AnchorProbe {
        if self.solver.is_too_complex() || self.solver.remaining_budget() == 0 {
            return AnchorProbe::Inconclusive;
        }

        let matches = self.solver.satisfiable(node, kind);
        if self.solver.is_too_complex() {
            return AnchorProbe::Inconclusive;
        }
        if matches {
            AnchorProbe::Matches
        } else {
            AnchorProbe::DoesNotMatch
        }
    }
}

fn emit_anchor_failure(
    solver: &SatisfiabilitySolver,
    culprit: &ConcreteCulprit,
    diag: &mut Diagnostics,
) {
    let ctx = solver.context();
    let node_pattern = culprit.node.node();
    let kind_name = culprit.kind_name(ctx);
    let span = culprit.span();
    let strict = strictest_anchor(node_pattern);

    // A leading anchor pins the first child; a trailing one the last. The two read
    // differently, so each gets its own message naming the boundary the grammar fixes.
    let Some(boundary) = Boundary::of(node_pattern) else {
        return emit_interior_anchor_failure(ctx, culprit, strict, diag);
    };
    let boundary_name = boundary.name();
    let boundary_verb = boundary.verb();
    let allowed = boundary.child_kinds(solver, culprit.kind);
    let wanted = boundary.pattern_label(&culprit.node, ctx);

    let demand = if strict {
        "with strict adjacency (`.!`)"
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
            "{demand}, no {kind_name} places this child {boundary_name}; \
             {} {kind_name} {} {}",
            indefinite_article(&kind_name),
            boundary_verb,
            render_kind_list(ctx, &allowed, "no fixed kind"),
        ),
    };

    let mut builder = diag
        .report(DiagnosticKind::UnsatisfiablePattern, span)
        .detail(detail);
    builder = builder.hint(anchor_fix_hint(&culprit.node, ctx, strict));
    builder.emit();
}

/// An anchor between two children — neither end is pinned, so explain the adjacency.
fn emit_interior_anchor_failure(
    ctx: AutomatonContext<'_>,
    culprit: &ConcreteCulprit,
    strict: bool,
    diag: &mut Diagnostics,
) {
    let kind_name = culprit.kind_name(ctx);
    let detail = format!("no {kind_name} places these children in this adjacency");
    diag.report(DiagnosticKind::UnsatisfiablePattern, culprit.span())
        .detail(detail)
        .hint(anchor_fix_hint(&culprit.node, ctx, strict))
        .emit();
}

/// The child kinds or their order are the obstacle (the anchors, if any, are not).
fn emit_arrangement_failure(
    ctx: AutomatonContext<'_>,
    culprit: &ConcreteCulprit,
    diag: &mut Diagnostics,
) {
    let kind_name = culprit.kind_name(ctx);
    let detail = format!("the grammar builds no {kind_name} with these children");
    let mut builder = diag
        .report(DiagnosticKind::UnsatisfiablePattern, culprit.span())
        .detail(detail);
    if let Some(allows) = describe_allowed_children(ctx, culprit.kind, &kind_name) {
        builder = builder.hint(allows);
    }
    builder = builder.hint(
        "each child is admissible here on its own — it is their combination or order \
         the grammar never produces",
    );
    builder.emit();
}

/// The primary solve proved the node impossible, but diagnostic refinement ran out
/// before it could distinguish anchors from child order. Keep the verdict precise
/// without pretending to know the specific obstacle.
fn emit_generic_failure(
    ctx: AutomatonContext<'_>,
    culprit: &ConcreteCulprit,
    diag: &mut Diagnostics,
) {
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
    ctx: AutomatonContext<'_>,
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
    Some(format!("{kind_name} allows {}", parts.join("; ")))
}

/// The fix hint: for a strict anchor, the soft form often matches; for a soft anchor,
/// dropping it does. Shows the rewritten node where it can be derived by swapping the
/// anchor token in the node's own source.
fn anchor_fix_hint(node: &Located<NodePattern>, ctx: AutomatonContext<'_>, strict: bool) -> String {
    if strict {
        match relaxed_anchor_text(node, ctx) {
            Some(soft) => format!(
                "`.!` allows nothing between — not even anonymous tokens or comments; \
                 the soft anchor `.` skips those: `{soft}`"
            ),
            None => "`.!` allows nothing between — not even anonymous tokens or comments; \
                 try the soft anchor `.`, which skips them"
                .to_string(),
        }
    } else {
        "`.` skips anonymous tokens and extras but never another named node; \
         drop the anchor to match these children in any positions"
            .to_string()
    }
}

/// The node's own source with `.!` rewritten to `.` — a concrete suggestion.
fn relaxed_anchor_text(node: &Located<NodePattern>, ctx: AutomatonContext<'_>) -> Option<String> {
    let range = node.node().text_range();
    let text = ctx.content(node.source());
    let slice = text.get(usize::from(range.start())..usize::from(range.end()))?;
    slice.contains(".!").then(|| slice.replace(".!", "."))
}

fn has_anchor(node: &NodePattern) -> bool {
    node.items().any(|item| matches!(item, SeqItem::Anchor(_)))
}

#[derive(Clone, Copy)]
enum Boundary {
    First,
    Last,
}

impl Boundary {
    fn of(node: &NodePattern) -> Option<Self> {
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

    fn pattern_label(
        self,
        node: &Located<NodePattern>,
        ctx: AutomatonContext<'_>,
    ) -> Option<String> {
        let pattern = match self {
            Self::First => node.node().items().find_map(seq_pattern)?,
            Self::Last => node.node().items().filter_map(seq_pattern).last()?,
        };
        pattern_label(&node.wrap(pattern), ctx)
    }
}

/// Whether any anchor in the node is strict — strict adjacency is the harsher demand,
/// so its explanation governs when both kinds are present.
fn strictest_anchor(node: &NodePattern) -> bool {
    node.items()
        .any(|item| matches!(item, SeqItem::Anchor(a) if a.is_strict()))
}

fn seq_pattern(item: SeqItem) -> Option<Pattern> {
    match item {
        SeqItem::Pattern(p) => Some(p),
        SeqItem::Anchor(_) => None,
    }
}

/// A short label for a child pattern: its node kind, its literal token, or `None`
/// when it is something compound (an alternation, a reference) better left unnamed.
fn pattern_label(located: &Located<Pattern>, ctx: AutomatonContext<'_>) -> Option<String> {
    match located.node() {
        Pattern::CapturedPattern(cap) => pattern_label(&located.wrap(cap.inner()?), ctx),
        Pattern::NodePattern(node) => {
            if node.is_any() {
                return Some("any named node".to_string());
            }
            root_kind(ctx, &located.wrap(node.clone())).map(|kind| render_kind(ctx, kind))
        }
        Pattern::TokenPattern(token) => {
            if token.is_any() {
                return Some("any node".to_string());
            }
            let value = token.value()?;
            Some(format!("`\"{}\"`", value.text()))
        }
        _ => None,
    }
}

/// The span of a node pattern's kind token, falling back to the whole node.
fn kind_span(node: &Located<NodePattern>) -> Span {
    let range = node
        .node()
        .kind_token()
        .map_or_else(|| node.node().text_range(), |token| token.text_range());
    node.span_of(range)
}

/// Render one kind: a named kind in backticks, an anonymous one as a quoted literal.
fn render_kind(ctx: AutomatonContext<'_>, kind: NodeKindId) -> String {
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
fn render_kind_list(ctx: AutomatonContext<'_>, kinds: &[NodeKindId], empty: &str) -> String {
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
