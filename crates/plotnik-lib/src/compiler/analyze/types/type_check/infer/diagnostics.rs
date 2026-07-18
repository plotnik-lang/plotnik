use std::collections::HashSet;

use crate::compiler::analyze::shape::RootExtent;
use crate::compiler::analyze::types::type_analysis::UnifyError;
use crate::compiler::analyze::types::type_description::describe_type;
use crate::compiler::analyze::types::type_shape::{PatternFlow, PatternShape, TypeId};
use crate::compiler::diagnostics::report::DiagnosticKind;
use crate::compiler::diagnostics::span::Span;
use crate::compiler::parse::ast::{
    AlternationPattern, CapturedPattern, Def, FieldPattern, Pattern, QuantifiedPattern,
};
use crate::compiler::parse::cst::{SyntaxKind, SyntaxNode, SyntaxToken};
use crate::core::Symbol;
use crate::core::utils::to_snake_case;

use super::InferVisitor;

struct ReferencedDefinition {
    name: String,
    span: Span,
    body_span: Span,
    capture_target: Option<String>,
}

impl InferVisitor<'_, '_> {
    pub(super) fn report_field_requires_single_node(
        &mut self,
        field: &FieldPattern,
        value: &Pattern,
    ) {
        let field_name = field
            .name()
            .map(|t| t.text().to_string())
            .unwrap_or_else(|| "grammar field".to_string());

        let related = self.referenced_definition(value);

        let mut builder = self
            .report(
                DiagnosticKind::GrammarFieldSequenceValue,
                value.text_range(),
            )
            .detail(field_name);
        if let Some(definition) = related {
            builder = builder.related_to(
                definition.span,
                format!("`{}` is defined here", definition.name),
            );
        }

        builder.emit();
    }

    fn referenced_definition(&self, value: &Pattern) -> Option<ReferencedDefinition> {
        let Pattern::DefRef(r) = value else {
            return None;
        };
        let name = r.name()?;
        let def_id = self
            .ctx
            .definitions
            .id_for_name(self.ctx.interner, name.text())?;
        let definition = self.ctx.definitions.definition(def_id);
        let source = definition.source();
        let body = definition.body();
        Some(ReferencedDefinition {
            name: name.text().to_string(),
            span: definition.span(),
            body_span: Span::new(source, body.text_range()),
            capture_target: first_result_capture_target(body),
        })
    }

    /// Report a captured quantifier whose no-value inner doesn't match exactly one
    /// node: there is no single node to bind (per element, for repeats).
    pub(super) fn report_quantified_capture_without_single_node(
        &mut self,
        quant: &QuantifiedPattern,
        inner_info: &PatternShape,
        capture_name: Option<Symbol>,
    ) {
        let capture_has_no_single_node =
            inner_info.root_extent == RootExtent::NotSingleNode && inner_info.flow.is_no_value();
        if !capture_has_no_single_node {
            return;
        }

        let op = self.quantifier_operator(quant);
        let inner = quant
            .inner()
            .expect("validated quantified pattern has an inner pattern");
        let inner_text = inline_source(inner.syntax());
        let referenced_definition = self.referenced_definition(&inner);
        let (detail, hint) = if let Some(capture_name) = capture_name {
            let capture_name = self.ctx.interner.resolve(capture_name);
            let suggested_capture = if op.starts_with('?') {
                format!("{capture_name}_value")
            } else {
                format!("{capture_name}_item")
            };
            let direct_target = first_result_capture_target(&inner);
            let capture_target = referenced_definition
                .as_ref()
                .and_then(|definition| definition.capture_target.as_deref())
                .or(direct_target.as_deref());
            let (unit, hint) = if op.starts_with('?') {
                (
                    "the optional pattern",
                    result_capture_hint(
                        referenced_definition.as_ref(),
                        capture_target,
                        &suggested_capture,
                        "make the optional pattern produce a record when it matches",
                    ),
                )
            } else {
                (
                    "one repetition",
                    result_capture_hint(
                        referenced_definition.as_ref(),
                        capture_target,
                        &suggested_capture,
                        "make each repetition produce a record",
                    ),
                )
            };
            (
                format!(
                    "`@{capture_name}` cannot collect `{inner_text}{op}` because {unit} does not produce one node or value"
                ),
                hint,
            )
        } else {
            let definition = enclosing_definition_name(quant);
            let subject = format!("definition `{definition}`");
            let suggested_capture = format!("{}_item", to_snake_case(&definition));
            let capture_target = first_result_capture_target(&inner);
            (
                format!(
                    "{subject} cannot collect `{inner_text}{op}` because one occurrence does not produce one node or value"
                ),
                result_capture_hint(
                    None,
                    capture_target.as_deref(),
                    &suggested_capture,
                    &format!("make each `{op}` occurrence produce a record"),
                ),
            )
        };
        let mut builder = self
            .report(DiagnosticKind::CaptureWithoutSingleNode, quant.text_range())
            .detail(detail)
            .hint(hint);
        if let Some(definition) = referenced_definition {
            builder = builder.related_to(
                definition.span,
                format!("`{}` is defined here", definition.name),
            );
        }
        builder.emit();
    }

    /// Report a capture whose no-value inner doesn't match exactly one node —
    /// whether several or possibly none, there is no single node to bind.
    /// Without this, the capture would silently bind an arbitrary node (or one
    /// per repeat), or dangle on an empty match.
    pub(super) fn report_capture_without_single_node(
        &mut self,
        inner: &Pattern,
        inner_info: &PatternShape,
        capture_name: Symbol,
    ) {
        if inner_info.root_extent != RootExtent::NotSingleNode || !inner_info.flow.is_no_value() {
            return;
        }

        let capture_name = self.ctx.interner.resolve(capture_name).to_string();
        let inner_text = inline_source(inner.syntax());
        let subject = match inner {
            Pattern::DefRef(_) => "the referenced definition",
            Pattern::Alternation(_) => "the alternation",
            _ => "the grouped pattern",
        };
        let detail = format!(
            "`@{capture_name}` cannot capture `{inner_text}` because {subject} does not match exactly one node"
        );
        let related = self.referenced_definition(inner);
        let suggested_capture = format!("{capture_name}_value");
        let direct_target = first_result_capture_target(inner);
        let capture_target = related
            .as_ref()
            .and_then(|definition| definition.capture_target.as_deref())
            .or(direct_target.as_deref());
        let hint = result_capture_hint(
            related.as_ref(),
            capture_target,
            &suggested_capture,
            "make the captured pattern produce a record",
        );
        let mut builder = self
            .report(DiagnosticKind::CaptureWithoutSingleNode, inner.text_range())
            .detail(detail)
            .hint(hint)
            .hint(format!("or remove `@{capture_name}`"));
        if let Some(definition) = related {
            builder = builder.related_to(
                definition.span,
                format!("`{}` is defined here", definition.name),
            );
        }
        builder.emit();
    }

    /// Report a captured reference whose definition has no value to capture.
    pub(super) fn report_capture_on_match_only_ref(
        &mut self,
        inner: &Pattern,
        inner_info: &PatternShape,
        capture_name: Symbol,
    ) -> bool {
        if !matches!(inner, Pattern::DefRef(_)) || !inner_info.flow.is_no_value() {
            return false;
        }

        let definition = self
            .referenced_definition(inner)
            .expect("resolved definition reference has definition provenance");
        let capture_name = self.ctx.interner.resolve(capture_name).to_string();
        let suggested_capture = format!("{capture_name}_value");
        let hint = result_capture_hint(
            Some(&definition),
            definition.capture_target.as_deref(),
            &suggested_capture,
            &format!("make `{}` produce a result", definition.name),
        );
        let mut builder = self
            .report(
                DiagnosticKind::MatchOnlyReferenceCapture,
                inner.text_range(),
            )
            .detail(format!(
                "`@{capture_name}` cannot capture `({})` because `{}` produces no result value",
                definition.name, definition.name
            ))
            .hint(hint)
            .hint(format!(
                "or remove `@{capture_name}` and capture a node pattern directly"
            ));
        builder = builder.related_to(
            definition.span,
            format!("`{}` is defined here", definition.name),
        );
        builder.emit();
        true
    }

    /// Report a repeat whose element is a reference that can match zero nodes.
    pub(super) fn report_nullable_repeat(&mut self, quant: &QuantifiedPattern, inner: &Pattern) {
        let op = self.quantifier_operator(quant);
        let definition = self
            .referenced_definition(inner)
            .expect("nullable-repeat diagnostic is only emitted for resolved references");
        let reference = format!("`({})`", definition.name);
        let quantified = inline_source(quant.syntax());
        let mut builder = self
            .report(DiagnosticKind::NullableRepeat, quant.text_range())
            .detail(format!(
                "`{quantified}` can repeat without matching a node because {reference} can match zero nodes"
            ))
            .hint(format!(
                "make every path through `{}` match at least one node, or remove `{op}`",
                definition.name
            ));
        builder = builder.related_to(
            definition.body_span,
            format!("this body of `{}` can match zero nodes", definition.name),
        );
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
        let collection = if op.starts_with('?') {
            "option"
        } else {
            "list"
        };
        let definition = enclosing_definition_name(quant);
        let inner = quant
            .inner()
            .expect("validated quantified pattern has an inner pattern");
        let inner_text = inline_source(inner.syntax());
        self.report(DiagnosticKind::UnnamedQuantifiedElement, quant.text_range())
            .detail(format!(
                "definition `{definition}` names the {collection}, but its element `{inner_text}` is {element_desc} without a type name"
            ))
            .hint(format!(
                "move `{inner_text}` into its own named definition, then apply `{op}` to a reference to that definition"
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
                "captures {} repeat with `{}` but are not collected into a list",
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
        let collection_name = fresh_capture_name(&taken);
        let quantified_text = quant.syntax().text().to_string();
        let fix_description = if op.starts_with('?') {
            format!("collect the optional record in `@{collection_name}`")
        } else {
            format!("collect one record per repetition in `@{collection_name}`")
        };
        self.report(
            DiagnosticKind::UncollectedQuantifiedCaptures,
            quant.text_range(),
        )
        .detail(detail)
        .fix(
            fix_description,
            format!("{quantified_text} @{collection_name}"),
        )
        .hint(if op.starts_with('?') {
            "append `@_` instead if the optional result should be discarded"
        } else {
            "append `@_` instead if the repeated result should be discarded"
        })
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

    pub(super) fn report_alternative_unify_error(&mut self, err: &UnifyError) {
        match err {
            UnifyError::IncompatibleFieldTypes {
                field,
                left_type,
                right_type,
                name_spans,
                fallback_span,
                ..
            } => {
                let field_name = self.ctx.interner.resolve(*field).to_string();
                let (primary, rest) = match name_spans.split_first() {
                    Some((first, rest)) => (first.0, rest),
                    None => (*fallback_span, &[][..]),
                };
                let left_type = self.describe_type(*left_type);
                let right_type = self.describe_type(*right_type);

                let related = rest
                    .iter()
                    .map(|&(site, type_id)| {
                        let label = format!(
                            "`@{field_name}` has type `{}` here",
                            self.describe_type(type_id)
                        );
                        (site, label)
                    })
                    .collect::<Vec<_>>();
                let mut builder = self
                    .ctx
                    .diag
                    .report(DiagnosticKind::IncompatibleCaptureTypes, primary)
                    .detail(format!(
                        "`@{field_name}` has incompatible types `{left_type}` and `{right_type}` across alternatives"
                    ));
                for (site, label) in related {
                    builder = builder.related_to(site, label);
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
            .expect("validated quantified pattern has an operator")
            .text()
            .to_string()
    }

    fn describe_type(&self, type_id: TypeId) -> String {
        describe_type(&self.ctx.type_ctx.in_progress(), self.ctx.interner, type_id)
    }
}

/// Collect captures that contribute result fields to one result scope.
fn direct_scope_capture_tokens(scope_root: &SyntaxNode, out: &mut Vec<SyntaxToken>) {
    for child in scope_root.children() {
        if let Some(captured_pattern) = CapturedPattern::cast(child.clone()) {
            if let Some(tok) = captured_pattern.capture().name() {
                out.push(tok);
            }
            if inner_captures_bubble_up(&captured_pattern) {
                direct_scope_capture_tokens(&child, out);
            }
            continue;
        }
        direct_scope_capture_tokens(&child, out);
    }
}

/// Find the result scope that would receive the suggested capture.
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
    CapturedPattern::cast(node.clone())
        .is_some_and(|captured_pattern| !inner_captures_bubble_up(&captured_pattern))
}

fn is_pattern_node(node: &SyntaxNode) -> bool {
    Pattern::cast(node.clone()).is_some()
}

/// Decide whether a capture exposes its inner result fields to the surrounding result scope.
/// Plain node captures do; structured captures, repetition captures, and discards contain them.
fn inner_captures_bubble_up(captured_pattern: &CapturedPattern) -> bool {
    if captured_pattern.capture().is_discard() {
        return false;
    }
    let mut inner = captured_pattern.inner();
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
fn fresh_capture_name(taken: &[String]) -> String {
    let taken: HashSet<&str> = taken.iter().map(String::as_str).collect();
    for base in ["items", "matches", "entries", "elements", "records"] {
        if !taken.contains(base) {
            return base.to_string();
        }
    }
    for suffix in 2.. {
        let candidate = format!("items_{suffix}");
        if !taken.contains(candidate.as_str()) {
            return candidate;
        }
    }
    unreachable!("an unbounded capture-name sequence always has a free name")
}

fn enclosing_definition_name(quant: &QuantifiedPattern) -> String {
    quant
        .syntax()
        .ancestors()
        .find_map(Def::cast)
        .expect("inferred quantified pattern belongs to a definition")
        .name()
        .expect("validated definition has a name")
        .text()
        .to_string()
}

fn first_result_capture_target(pattern: &Pattern) -> Option<String> {
    if let Some(target) = pattern
        .children()
        .find_map(|child| first_result_capture_target(&child))
    {
        return Some(target);
    }

    matches!(
        pattern,
        Pattern::NamedNodePattern(_) | Pattern::AnonymousNodePattern(_) | Pattern::NodeWildcard(_)
    )
    .then(|| inline_source(pattern.syntax()))
}

fn result_capture_hint(
    definition: Option<&ReferencedDefinition>,
    target: Option<&str>,
    capture_name: &str,
    outcome: &str,
) -> String {
    match (definition, target) {
        (Some(definition), Some(target)) => format!(
            "{outcome} by changing `{target}` to `{target} @{capture_name}` in definition `{}`",
            definition.name
        ),
        (None, Some(target)) => {
            format!("{outcome} by changing `{target}` to `{target} @{capture_name}`")
        }
        (Some(definition), None) => format!(
            "make `{}` produce a result before collecting or capturing its value",
            definition.name
        ),
        (None, None) => format!(
            "{outcome} by placing `@{capture_name}` after the node that should provide its result"
        ),
    }
}

fn inline_source(node: &SyntaxNode) -> String {
    let mut result = String::new();
    let mut pending_space = false;
    let mut after_line_comment = false;

    for token in node
        .descendants_with_tokens()
        .filter_map(|element| element.into_token())
    {
        match token.kind() {
            SyntaxKind::Whitespace => pending_space = true,
            SyntaxKind::Newline if after_line_comment => {
                result.push_str(" ⏎ ");
                pending_space = false;
                after_line_comment = false;
            }
            SyntaxKind::Newline => pending_space = true,
            SyntaxKind::LineComment => {
                if pending_space && !result.is_empty() {
                    result.push(' ');
                }
                result.push_str(token.text());
                pending_space = false;
                after_line_comment = true;
            }
            _ => {
                if pending_space && !result.is_empty() {
                    result.push(' ');
                }
                result.push_str(token.text());
                pending_space = false;
                after_line_comment = false;
            }
        }
    }

    result
}
