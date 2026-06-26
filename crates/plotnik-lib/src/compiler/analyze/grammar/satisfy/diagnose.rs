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
use super::engine::Satisfier;
use super::{Mode, collect_goals, root_kind};

/// Emit the impossibility diagnostic for a failed root goal.
pub(super) fn report(
    satisfier: &mut Satisfier,
    node: &Located<NodePattern>,
    kind: NodeKindId,
    diag: &mut Diagnostics,
) {
    let mut visited = HashSet::new();
    let culprit = locate(satisfier, node.clone(), kind, &mut visited);

    let ctx = satisfier.context();

    // A single-valued field bound more than once is the whole obstacle on its own —
    // a sharper thing to say than a vague "combination the grammar never produces".
    if let Some((field, count)) = repeated_single_field(ctx, &culprit) {
        return emit_repeated_field_failure(ctx, &culprit, &field, count, diag);
    }

    let anchors_to_blame =
        has_anchor(culprit.node.node()) && relaxing_anchors_satisfies(satisfier, &culprit);

    if anchors_to_blame {
        emit_anchor_failure(satisfier, &culprit, diag);
    } else {
        emit_arrangement_failure(ctx, &culprit, diag);
    }
}

/// Report an impossible wildcard parent `(_ …)`: no kind the grammar builds takes the
/// children it constrains. A wildcard fixes no kind of its own, so there is no single
/// node to blame — the obstacle is that no production anywhere realizes this child list.
pub(super) fn report_wildcard(node: &Located<NodePattern>, diag: &mut Diagnostics) {
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
fn repeated_single_field(ctx: AutomatonContext<'_>, culprit: &Culprit) -> Option<(String, usize)> {
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
    culprit: &Culprit,
    field: &str,
    count: usize,
    diag: &mut Diagnostics,
) {
    let kind_name = render_kind(ctx, culprit.kind);
    let article = indefinite_article(&kind_name);
    let detail = format!(
        "{article} {kind_name} has one `{field}`, but this pattern binds `{field}` {count} times"
    );
    diag.report(DiagnosticKind::UnsatisfiablePattern, kind_span(&culprit.node))
        .detail(detail)
        .hint(format!("keep a single `{field}:` child"))
        .emit();
}

/// The node a diagnostic points at, plus its resolved kind.
struct Culprit {
    node: Located<NodePattern>,
    kind: NodeKindId,
}

/// Walk down from an unsatisfiable node to the deepest node whose children are each
/// satisfiable on their own — the point where the arrangement, not any one child, is
/// what the grammar rejects.
fn locate(
    satisfier: &mut Satisfier,
    node: Located<NodePattern>,
    kind: NodeKindId,
    visited: &mut HashSet<SyntaxNode>,
) -> Culprit {
    if !visited.insert(node.node().syntax().clone()) {
        // A recursive definition folded back onto a node already being blamed.
        return Culprit { node, kind };
    }

    let ctx = satisfier.context();
    let mut children = Vec::new();
    for child in node.node().children() {
        collect_goals(&node.wrap(child), Mode::Required, ctx, &mut children);
    }

    for child in children {
        if !satisfier.satisfiable(&child.node, child.kind) {
            return locate(satisfier, child.node, child.kind, visited);
        }
    }

    Culprit { node, kind }
}

/// Re-solve the culprit with every gap widened to "any node may intervene". A fresh
/// satisfier keeps the relaxed automata out of the real run's memo. If this matches
/// while the strict solve did not, the anchors are provably the only obstacle.
fn relaxing_anchors_satisfies(satisfier: &Satisfier, culprit: &Culprit) -> bool {
    let mut relaxed = Satisfier::new(
        satisfier.context(),
        true,
        satisfier.max_depth(),
        satisfier.step_budget(),
    );
    relaxed.satisfiable(&culprit.node, culprit.kind)
}

fn emit_anchor_failure(satisfier: &Satisfier, culprit: &Culprit, diag: &mut Diagnostics) {
    let ctx = satisfier.context();
    let node = culprit.node.node();
    let kind_name = render_kind(ctx, culprit.kind);
    let span = kind_span(&culprit.node);
    let strict = strictest_anchor(node);

    // A leading anchor pins the first child; a trailing one the last. The two read
    // differently, so each gets its own message naming the boundary the grammar fixes.
    let (boundary, allowed, wanted) = if leads_with_anchor(node) {
        ("first", satisfier.first_child_kinds(culprit.kind), first_pattern_label(&culprit.node, ctx))
    } else if ends_with_anchor(node) {
        ("last", satisfier.last_child_kinds(culprit.kind), last_pattern_label(&culprit.node, ctx))
    } else {
        return emit_interior_anchor_failure(ctx, culprit, strict, diag);
    };

    let demand = if strict {
        "with strict adjacency (`.!`)"
    } else {
        "after the soft anchor (`.`)"
    };
    let detail = match &wanted {
        Some(want) => format!(
            "{demand}, the {boundary} child of {kind_name} must be {want}, \
             but {kind_name} {} {}",
            boundary_verb(boundary),
            render_kind_list(ctx, &allowed, "other kinds"),
        ),
        None => format!(
            "{demand}, no {kind_name} places this child {boundary}; \
             {} {kind_name} {} {}",
            indefinite_article(&kind_name),
            boundary_verb(boundary),
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
    culprit: &Culprit,
    strict: bool,
    diag: &mut Diagnostics,
) {
    let kind_name = render_kind(ctx, culprit.kind);
    let detail = format!("no {kind_name} places these children in this adjacency");
    diag.report(DiagnosticKind::UnsatisfiablePattern, kind_span(&culprit.node))
        .detail(detail)
        .hint(anchor_fix_hint(&culprit.node, ctx, strict))
        .emit();
}

/// The child kinds or their order are the obstacle (the anchors, if any, are not).
fn emit_arrangement_failure(ctx: AutomatonContext<'_>, culprit: &Culprit, diag: &mut Diagnostics) {
    let kind_name = render_kind(ctx, culprit.kind);
    let detail = format!("the grammar builds no {kind_name} with these children");
    let mut builder = diag
        .report(DiagnosticKind::UnsatisfiablePattern, kind_span(&culprit.node))
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

fn leads_with_anchor(node: &NodePattern) -> bool {
    matches!(node.items().next(), Some(SeqItem::Anchor(_)))
}

fn ends_with_anchor(node: &NodePattern) -> bool {
    matches!(node.items().last(), Some(SeqItem::Anchor(_)))
}

/// Whether any anchor in the node is strict — strict adjacency is the harsher demand,
/// so its explanation governs when both kinds are present.
fn strictest_anchor(node: &NodePattern) -> bool {
    node.items()
        .any(|item| matches!(item, SeqItem::Anchor(a) if a.is_strict()))
}

fn boundary_verb(boundary: &str) -> &'static str {
    if boundary == "first" { "begins with" } else { "ends with" }
}

/// The label of the first/last child pattern (`identifier`, `"+"`), for "must be …".
fn first_pattern_label(node: &Located<NodePattern>, ctx: AutomatonContext<'_>) -> Option<String> {
    let pattern = node.node().items().find_map(seq_pattern)?;
    pattern_label(&node.wrap(pattern), ctx)
}

fn last_pattern_label(node: &Located<NodePattern>, ctx: AutomatonContext<'_>) -> Option<String> {
    let pattern = node.node().items().filter_map(seq_pattern).last()?;
    pattern_label(&node.wrap(pattern), ctx)
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
