use rowan::TextRange;

use crate::compiler::analyze::types::RootExtent;
use crate::compiler::analyze::types::type_shape::{PatternFlow, PatternShape};
use crate::compiler::diagnostics::report::DiagnosticKind;
use crate::compiler::diagnostics::source::SourceId;
use crate::compiler::diagnostics::span::Span;
use crate::compiler::parse::ast::{
    AlternationPattern, CapturedPattern, FieldPattern, Pattern, QuantifiedPattern,
};
use crate::compiler::parse::cst::{SyntaxNode, SyntaxToken};

use super::super::unify::UnifyError;
use super::InferVisitor;

impl InferVisitor<'_, '_> {
    pub(super) fn report_field_requires_single_node(
        &mut self,
        field: &FieldPattern,
        value: &Pattern,
    ) {
        let field_name = field
            .name()
            .map(|t| t.text().to_string())
            .unwrap_or_else(|| "field".to_string());

        let related = self.referenced_definition_range(value);

        let mut builder = self
            .report(DiagnosticKind::FieldSequenceValue, value.text_range())
            .detail(field_name);
        if let Some((src, range)) = related {
            builder = builder.related_to(Span::new(src, range), "defined here");
        }

        builder.emit();
    }

    fn referenced_definition_range(&self, value: &Pattern) -> Option<(SourceId, TextRange)> {
        let Pattern::DefRef(r) = value else {
            return None;
        };
        let name = r.name()?;
        let (source, body) = self.ctx.symbol_table.definition(name.text())?;
        Some((source, body.text_range()))
    }

    /// Report a captured quantifier whose no-value inner doesn't match exactly one
    /// node: there is no single node to bind (per element, for repeats).
    pub(super) fn report_quantified_capture_without_single_node(
        &mut self,
        quant: &QuantifiedPattern,
        inner_info: &PatternShape,
    ) {
        let capture_has_no_single_node =
            inner_info.root_extent == RootExtent::Other && inner_info.flow.is_no_value();
        if !capture_has_no_single_node {
            return;
        }

        let op = self.quantifier_operator(quant);
        let (detail, hint) = if op.starts_with('?') {
            (
                format!(
                    "this `{op}` group doesn't match exactly one node, so there is no single node to bind"
                ),
                "capture individual nodes inside the group: `{(a) @a (b) @b}? @x`".to_string(),
            )
        } else {
            (
                format!(
                    "one repeat of this `{op}` group doesn't match exactly one node, so there is no single node to bind per element"
                ),
                format!("add internal captures: `{{(a) @a (b) @b}}{op} @items`"),
            )
        };
        self.report(DiagnosticKind::CaptureWithoutSingleNode, quant.text_range())
            .detail(detail)
            .hint(hint)
            .emit();
    }

    /// Report a capture whose no-value inner doesn't match exactly one node —
    /// whether several or possibly none, there is no single node to bind.
    /// Without this, the capture would silently bind an arbitrary node (or one
    /// per repeat), or dangle on an empty match.
    pub(super) fn report_capture_without_single_node(
        &mut self,
        inner: &Pattern,
        inner_info: &PatternShape,
    ) {
        if inner_info.root_extent != RootExtent::Other || !inner_info.flow.is_no_value() {
            return;
        }

        let (detail, hint) = match inner {
            Pattern::DefRef(_) => (
                "the referenced definition doesn't match exactly one node, so there is no single node to bind",
                "capture nodes inside the definition, or drop this capture",
            ),
            Pattern::Alternation(_) => (
                "this alternation doesn't match exactly one node, so there is no single node to bind",
                "capture inside the alternatives, or drop this capture",
            ),
            _ => (
                "this group doesn't match exactly one node, so there is no single node to bind",
                "capture individual nodes inside the group: `{(a) @a (b) @b} @x`",
            ),
        };

        let related = self.referenced_definition_range(inner);
        let mut builder = self
            .report(DiagnosticKind::CaptureWithoutSingleNode, inner.text_range())
            .detail(detail)
            .hint(hint);
        if let Some((src, range)) = related {
            builder = builder.related_to(Span::new(src, range), "defined here");
        }
        builder.emit();
    }

    /// Report a captured reference whose definition has no value to capture.
    pub(super) fn report_capture_on_match_only_ref(
        &mut self,
        inner: &Pattern,
        inner_info: &PatternShape,
    ) -> bool {
        if !matches!(inner, Pattern::DefRef(_)) || !inner_info.flow.is_no_value() {
            return false;
        }

        let Some((src, range)) = self.referenced_definition_range(inner) else {
            return false;
        };
        let mut builder = self
            .report(
                DiagnosticKind::MatchOnlyReferenceCapture,
                inner.text_range(),
            )
            .detail(
                "the referenced definition produces no value; add a capture inside it or capture a node pattern directly",
            );
        builder = builder.related_to(Span::new(src, range), "defined here");
        builder.emit();
        true
    }

    /// Report a repeat whose element is a reference that can match zero nodes.
    pub(super) fn report_nullable_repeat(&mut self, quant: &QuantifiedPattern, inner: &Pattern) {
        let op = self.quantifier_operator(quant);
        let related = self.referenced_definition_range(inner);
        let mut builder = self
            .report(DiagnosticKind::NullableRepeat, quant.text_range())
            .detail(format!(
                "the referenced definition can match zero nodes, but a `{op}` repeat must consume input on every iteration — its empty case could never occur here"
            ))
            .hint("make the definition consume at least one node, or drop the quantifier");
        if let Some((src, range)) = related {
            builder = builder.related_to(Span::new(src, range), "can match zero nodes here");
        }
        builder.emit();
    }

    /// Report a quantifier-rooted definition whose element shape has no name
    /// source. The definition names the collection (the list/option type);
    /// naming the element takes its own definition.
    pub(super) fn report_unnamed_quantified_element(
        &mut self,
        quant: &QuantifiedPattern,
        element_desc: &str,
    ) {
        let op = self.quantifier_operator(quant);
        let (collection, example_op) = if op.starts_with('?') {
            ("option", op.as_str())
        } else {
            ("list", op.as_str())
        };
        self.report(DiagnosticKind::UnnamedQuantifiedElement, quant.text_range())
            .detail(format!(
                "the definition names the {collection} itself, so each `{op}` element — {element_desc} — is left without a type name"
            ))
            .hint(format!(
                "name the element type in its own definition, then quantify a reference to it: `Elem = ...` and `(Elem){example_op}`"
            ))
            .emit();
    }

    /// Report bubbling captures that need a repetition or optional capture to
    /// package one record value per quantifier occurrence.
    pub(super) fn report_uncollected_quantified_captures(
        &mut self,
        quant: &QuantifiedPattern,
        inner_info: &PatternShape,
    ) {
        let PatternFlow::Fields(type_id) = &inner_info.flow else {
            return;
        };
        let type_ctx = self.ctx.type_ctx.in_progress();
        let fields = type_ctx.expect_record_fields(*type_id);
        if fields.is_empty() {
            return;
        }
        let raw_names: Vec<String> = fields
            .keys()
            .map(|s| self.ctx.interner.resolve(*s).to_string())
            .collect();

        let op = self.quantifier_operator(quant);
        let captures_str = raw_names
            .iter()
            .map(|n| format!("`@{}`", n))
            .collect::<Vec<_>>()
            .join(", ");
        let detail = if op.starts_with('?') {
            format!(
                "captures {} skip together with `{}` but nothing collects them",
                captures_str, op
            )
        } else {
            format!(
                "captures {} repeat with `{}` but aren't collected into a list",
                captures_str, op
            )
        };

        // The suggestion lands beside sibling captures, so avoid names bound there.
        let mut taken = raw_names;
        let scope_root = enclosing_scope_root(quant.syntax());
        let mut scope_tokens = Vec::new();
        direct_scope_capture_tokens(&scope_root, &mut scope_tokens);
        taken.extend(
            scope_tokens
                .iter()
                .filter_map(|tok| tok.text().get(1..).map(str::to_owned)),
        );
        let placeholder = fresh_capture_name(&taken);
        let brackets = capture_brackets(quant);
        let hint = if op.starts_with('?') {
            format!(
                "add an optional capture so the group becomes an option of one record: `{}{} @{}`",
                brackets, op, placeholder
            )
        } else {
            format!(
                "add a repetition capture so each repeat becomes one record in a list: `{}{} @{}`",
                brackets, op, placeholder
            )
        };
        self.report(
            DiagnosticKind::UncollectedQuantifiedCaptures,
            quant.text_range(),
        )
        .detail(detail)
        .hint(hint)
        .hint(format!(
            "or discard the captures if only the structure matters: `{}{} @_`",
            brackets, op
        ))
        .emit();
    }

    /// Warn when labels appear in a fields context: they have no output effect,
    /// and captures from the alternatives merge into the enclosing result.
    pub(super) fn report_unused_alternative_labels(&mut self, alternation: &AlternationPattern) {
        self.report(
            DiagnosticKind::UnusedAlternativeLabels,
            alternation.text_range(),
        )
        .detail("captures from the alternatives merge into the enclosing result")
        .emit();
    }

    pub(super) fn report_alternative_unify_error(
        &mut self,
        alternation: &SyntaxNode,
        err: &UnifyError,
    ) {
        match err {
            UnifyError::IncompatibleTypes { field } => {
                let field_name = self.ctx.interner.resolve(*field).to_string();
                let sites = capture_sites(alternation, &field_name);
                let source = self.source;
                let (primary, rest) = match sites.split_first() {
                    Some((first, rest)) => (*first, rest),
                    None => (alternation.text_range(), &[] as &[TextRange]),
                };

                let mut builder = self
                    .report(DiagnosticKind::IncompatibleCaptureTypes, primary)
                    .detail(field_name);
                for &site in rest {
                    builder =
                        builder.related_to(Span::new(source, site), "and a different type here");
                }
                builder
                    .hint("make every alternative produce the same type, or label the alternatives for a variant type")
                    .emit();
            }
        }
    }

    fn quantifier_operator(&self, quant: &QuantifiedPattern) -> String {
        quant
            .operator()
            .map(|t| t.text().to_string())
            .unwrap_or_else(|| "*".to_string())
    }
}

/// The bracket shorthand for a capture hint, matching the shape the user wrote:
/// `[...]` for alternations, `(...)` for nodes and references, and `{...}` for
/// sequences and other groups.
fn capture_brackets(quant: &QuantifiedPattern) -> &'static str {
    match quant.inner() {
        Some(Pattern::Alternation(_)) => "[...]",
        Some(Pattern::DefRef(_) | Pattern::NodePattern(_)) => "(...)",
        _ => "{...}",
    }
}

/// Find same-named captures that belong to the alternation's output scope.
/// Nested structured-capture scopes are excluded because their fields cannot
/// conflict here.
fn capture_sites(alternation: &SyntaxNode, field_name: &str) -> Vec<TextRange> {
    let mut tokens = Vec::new();
    direct_scope_capture_tokens(alternation, &mut tokens);
    tokens
        .into_iter()
        .filter(|tok| tok.text().get(1..) == Some(field_name))
        .map(|tok| tok.text_range())
        .collect()
}

/// Collect captures that contribute fields to one output scope.
fn direct_scope_capture_tokens(scope_root: &SyntaxNode, out: &mut Vec<SyntaxToken>) {
    for child in scope_root.children() {
        if let Some(cap) = CapturedPattern::cast(child.clone()) {
            if let Some(tok) = cap.name() {
                out.push(tok);
            }
            if inner_captures_bubble_up(&cap) {
                direct_scope_capture_tokens(&child, out);
            }
            continue;
        }
        direct_scope_capture_tokens(&child, out);
    }
}

/// Find the output scope that would receive the suggested capture.
fn enclosing_scope_root(node: &SyntaxNode) -> SyntaxNode {
    let mut root = node.clone();
    for ancestor in node.ancestors().skip(1) {
        if opens_nested_scope(&ancestor) {
            break;
        }
        if !is_pattern_node(&ancestor) {
            break;
        }
        root = ancestor;
    }
    root
}

fn opens_nested_scope(node: &SyntaxNode) -> bool {
    CapturedPattern::cast(node.clone()).is_some_and(|cap| !inner_captures_bubble_up(&cap))
}

fn is_pattern_node(node: &SyntaxNode) -> bool {
    Pattern::cast(node.clone()).is_some()
}

/// Decide whether a capture exposes its inner fields to the surrounding output scope.
/// Plain node captures do; structured captures, repetition captures, and discards contain them.
fn inner_captures_bubble_up(cap: &CapturedPattern) -> bool {
    if cap.is_discard() {
        return false;
    }
    let mut inner = cap.inner();
    loop {
        match inner {
            // A field constraint navigates to a child; it does not create a scope.
            Some(Pattern::FieldPattern(f)) => inner = f.value(),
            Some(Pattern::QuantifiedPattern(q)) => {
                if q.is_repeating() {
                    return false;
                }
                inner = q.inner();
            }
            Some(Pattern::SeqPattern(_) | Pattern::Alternation(_)) => return false,
            _ => return true,
        }
    }
}

/// Pick a suggestion that will not collide with captures already in scope.
fn fresh_capture_name(taken: &[String]) -> &'static str {
    ["items", "matches", "entries", "elements", "records"]
        .into_iter()
        .find(|candidate| !taken.iter().any(|t| t == candidate))
        .unwrap_or("items")
}
