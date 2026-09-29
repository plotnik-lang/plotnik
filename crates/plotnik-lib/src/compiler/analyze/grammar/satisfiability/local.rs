use std::collections::HashSet;

use super::super::format::format_list;
use super::super::model::{KindConstraint, Model, NodeViolation, PatternId, PatternKind};
use crate::compiler::analyze::Located;
use crate::compiler::diagnostics::report::{DiagnosticKind, Span};
use crate::compiler::parse::ast::{self, QuantifierKind};
use crate::core::{NodeFieldId, NodeKindId};

use crate::compiler::diagnostics::report::{DiagnosticBuilder, Diagnostics};
use crate::core::grammar::Grammar;

pub(super) struct LocalDiagnostics<'a> {
    grammar: &'a Grammar,
    diag: &'a mut Diagnostics,
    model: &'a Model,
    reported: bool,
    reported_nodes: &'a mut HashSet<PatternId>,
    visited: HashSet<(PatternId, Option<ParentNode>, Option<FieldRef>)>,
}

impl LocalDiagnostics<'_> {
    /// Explain simple structural contradictions only after the solver has rejected
    /// the enclosing pattern. These summaries choose wording, never admission.
    pub(super) fn report(
        grammar: &Grammar,
        model: &Model,
        pattern: PatternId,
        diag: &mut Diagnostics,
        reported_nodes: &mut HashSet<PatternId>,
    ) -> bool {
        let mut reporter = LocalDiagnostics {
            grammar,
            model,
            diag,
            reported: false,
            reported_nodes,
            visited: HashSet::new(),
        };
        reporter.walk(pattern, None, None);
        reporter.reported
    }

    fn walk(&mut self, pattern: PatternId, parent: Option<ParentNode>, field: Option<FieldRef>) {
        if !self.visited.insert((pattern, parent, field)) {
            return;
        }

        match &self.model.pattern(pattern).kind {
            PatternKind::Node(node) => {
                if let Some(parent) = parent {
                    self.child(pattern, node.kind, &parent, field.as_ref());
                }
                if self.reported_nodes.contains(&pattern) {
                    self.reported = true;
                    return;
                }

                let before = self.diag.len();
                let origin = self.model.node_origin(pattern);
                let KindConstraint::Exact(kind) = node.kind else {
                    return;
                };
                let token = origin
                    .node()
                    .kind_token()
                    .expect("resolved node has a kind");
                let ctx = ParentNode {
                    id: kind,
                    span: origin.span_of(token.text_range()),
                };
                match node.violation {
                    Some(NodeViolation::PredicateOnNonLeaf) => {
                        self.emit_predicate_on_non_leaf(&origin, &ctx)
                    }
                    Some(NodeViolation::FieldNotOnKind(field)) => {
                        let name = self
                            .grammar
                            .field_name(field.id)
                            .expect("resolved field has a name");
                        self.emit_negated_field_not_on_node(field.span, name, &ctx);
                    }
                    None => {}
                }
                for pattern in node.children.iter().filter_map(|item| item.pattern()) {
                    self.walk(pattern, Some(ctx), None);
                }
                if self.diag.len() != before {
                    self.reported_nodes.insert(pattern);
                }
            }
            PatternKind::Field(id, inner) => {
                let origin = self.model.pattern(pattern).syntax();
                let ast::Pattern::FieldPattern(field) = origin.node() else {
                    unreachable!("resolved field originates in field syntax");
                };
                let token = field.name().expect("resolved field has a name");
                self.walk(
                    *inner,
                    parent,
                    Some(FieldRef {
                        id: *id,
                        span: origin.span_of(token.text_range()),
                    }),
                );
            }
            PatternKind::Sequence(items) => {
                for pattern in items.iter().filter_map(|item| item.pattern()) {
                    // A group does not carry its outer field into its siblings.
                    self.walk(pattern, parent, None);
                }
            }
            PatternKind::Repeat(repeat) if repeat.kind == QuantifierKind::OneOrMore => {
                self.walk(repeat.inner, parent, field)
            }
            PatternKind::Reference(definition) => {
                self.walk(self.model.definition(*definition), parent, field)
            }
            PatternKind::Leaf(kind) => {
                if let Some(parent) = parent {
                    self.child(pattern, *kind, &parent, field.as_ref());
                }
            }
            PatternKind::Alternative(_) | PatternKind::Repeat(_) => {}
        }
    }

    fn child(
        &mut self,
        pattern: PatternId,
        kind: KindConstraint,
        parent: &ParentNode,
        field: Option<&FieldRef>,
    ) {
        let origin = self.model.pattern(pattern).syntax();
        let span = match origin.node() {
            ast::Pattern::NamedNodePattern(node) if !node.is_any() => origin.span_of(
                node.kind_token()
                    .expect("resolved node has a kind")
                    .text_range(),
            ),
            ast::Pattern::AnonymousNodePattern(node) => origin.span_of(
                node.value()
                    .expect("resolved token has a value")
                    .text_range(),
            ),
            _ => origin.span_of(origin.node().text_range()),
        };
        if let Some(field) = field {
            let name = self
                .grammar
                .field_name(field.id)
                .expect("resolved field has a name");
            if !self.grammar.has_field(parent.id(), field.id) {
                self.emit_field_not_on_node(field.span, name, parent);
                return;
            }

            let types = self.grammar.valid_field_types(parent.id(), field.id);
            let accepted = match kind {
                KindConstraint::Exact(kind) => types
                    .iter()
                    .any(|&candidate| self.accepts_kind(candidate, kind)),
                KindConstraint::AnyNamed => {
                    types.is_empty()
                        || types
                            .iter()
                            .any(|&kind| !self.grammar.is_anonymous_node(kind))
                }
                KindConstraint::AnyNode | KindConstraint::Unconstrained => true,
            };
            if !accepted {
                let value = match kind {
                    KindConstraint::Exact(kind) => format!(
                        "`{}`",
                        self.grammar
                            .node_kind(kind)
                            .expect("resolved kind has a name")
                    ),
                    _ => "a named node".to_string(),
                };
                self.emit_invalid_field_value(span, &value, parent, field);
            }
            return;
        }

        if !matches!(origin.node(), ast::Pattern::NamedNodePattern(_))
            || matches!(kind, KindConstraint::Unconstrained)
        {
            return;
        }

        if self.grammar.is_token(parent.id()) {
            self.emit_child_under_leaf_token(span, parent);
            return;
        }

        let KindConstraint::Exact(kind) = kind else {
            return;
        };
        let accepted = self.grammar.is_extra(kind)
            || self
                .child_kinds(parent.id())
                .any(|candidate| self.accepts_kind(candidate, kind));
        if !accepted {
            self.emit_invalid_child(span, kind, parent);
        }
    }

    fn emit_predicate_on_non_leaf(
        &mut self,
        node: &Located<ast::NamedNodePattern>,
        ctx: &ParentNode,
    ) {
        let predicate = node
            .node()
            .predicate()
            .expect("leaf restriction comes from a predicate");
        let name = ctx.name(self.grammar);
        let text = predicate.syntax().text();
        self.diag
            .report(
                DiagnosticKind::PredicateOnNonLeaf,
                node.span_of(predicate.syntax().text_range()),
            )
            .detail(format!(
                "predicate `{name} {text}` cannot test `{name}` because this node kind can contain children"
            ))
            .related_to(ctx.span(), format!("`{name}` starts here"))
            .hint(format!(
                "match the children of `{name}` instead of testing the whole node's text"
            ))
            .emit();
        self.reported = true;
    }

    fn field_error(
        &mut self,
        span: Span,
        field_name: &str,
        ctx: &ParentNode,
    ) -> DiagnosticBuilder<'_> {
        self.reported = true;
        let parent_name = ctx.name(self.grammar);
        self.diag
            .report(DiagnosticKind::GrammarFieldNotOnNodeKind, span)
            .detail(format!(
                "`{field_name}` is not a grammar field of `{parent_name}`"
            ))
            .related_to(ctx.span(), format!("`{parent_name}` starts here"))
    }

    fn emit_negated_field_not_on_node(&mut self, span: Span, field_name: &str, ctx: &ParentNode) {
        self.field_error(span, field_name, ctx)
            .hint(format!("remove `-{field_name}`"))
            .emit();
    }

    fn emit_field_not_on_node(&mut self, span: Span, field_name: &str, ctx: &ParentNode) {
        let grammar = self.grammar;
        let valid_fields = grammar.fields_for_node_kind(ctx.id());
        let parent_name = ctx.name(grammar);
        let mut builder = self.field_error(span, field_name, ctx);
        if valid_fields.is_empty() {
            builder
                .hint(format!("`{parent_name}` has no grammar fields"))
                .emit();
            return;
        }

        builder = builder.hint(format!(
            "valid grammar fields for `{parent_name}`: {}",
            format_list(&valid_fields, 5)
        ));
        builder.emit();
    }

    fn emit_invalid_child(&mut self, span: Span, child_id: NodeKindId, ctx: &ParentNode) {
        self.reported = true;
        let child_name = self
            .grammar
            .node_kind(child_id)
            .expect("resolved child must have a name")
            .to_string();
        let parent_name = ctx.name(self.grammar).to_string();
        let parent_span = ctx.span();
        let hint = self.child_hint(ctx.id(), &parent_name);

        self.diag
            .report(DiagnosticKind::InvalidChildType, span)
            .detail(format!(
                "`{child_name}` cannot be a child of `{parent_name}`"
            ))
            .related_to(parent_span, format!("`{parent_name}` starts here"))
            .hint(hint)
            .emit();
    }

    fn emit_child_under_leaf_token(&mut self, span: Span, ctx: &ParentNode) {
        self.reported = true;
        let parent_name = ctx.name(self.grammar).to_string();
        let parent_span = ctx.span();

        self.diag
            .report(DiagnosticKind::ChildUnderLeafToken, span)
            .detail(&parent_name)
            .related_to(parent_span, format!("`{}`", parent_name))
            .hint(format!(
                "a leaf token's content is its text. Match `({0})` directly or add a text predicate to `({0})`",
                parent_name
            ))
            .emit();
    }

    // An unlabelled query can also match a labelled child.
    fn child_kinds(&self, parent_id: NodeKindId) -> impl Iterator<Item = NodeKindId> + '_ {
        self.grammar
            .valid_child_types(parent_id)
            .iter()
            .copied()
            .chain(
                self.grammar
                    .field_ids_for_node_kind(parent_id)
                    .iter()
                    .flat_map(move |&field| {
                        self.grammar
                            .valid_field_types(parent_id, field)
                            .iter()
                            .copied()
                    }),
            )
    }

    fn accepts_kind(&self, candidate: NodeKindId, kind: NodeKindId) -> bool {
        candidate == kind || self.grammar.collect_subtypes(candidate).contains(&kind)
    }

    fn child_hint(&self, parent_id: NodeKindId, parent_name: &str) -> String {
        let kinds = self
            .child_kinds(parent_id)
            .chain(self.grammar.extra_node_kinds().iter().copied());
        let names = self.concrete_kinds(kinds);
        if names.is_empty() {
            return format!("`{parent_name}` has no named children");
        }
        format!(
            "named child kinds of `{parent_name}`: {}",
            format_list(&names, 8)
        )
    }

    fn concrete_kinds(&self, kinds: impl Iterator<Item = NodeKindId>) -> Vec<&str> {
        let mut names = Vec::new();
        for kind in kinds {
            let candidates = if self.grammar.is_supertype(kind) {
                self.grammar.collect_subtypes(kind).into_iter().collect()
            } else {
                vec![kind]
            };
            for candidate in candidates {
                if !self.grammar.is_supertype(candidate)
                    && !self.grammar.is_anonymous_node(candidate)
                {
                    names.push(
                        self.grammar
                            .node_kind(candidate)
                            .expect("retained kind has a name"),
                    );
                }
            }
        }
        names.sort_unstable();
        names.dedup();
        names
    }

    fn emit_invalid_field_value(
        &mut self,
        span: Span,
        value: &str,
        ctx: &ParentNode,
        field: &FieldRef,
    ) {
        self.reported = true;
        let name = self
            .grammar
            .field_name(field.id)
            .expect("resolved field has a name");
        let hint = self.field_value_hint(ctx.id(), field.id, name);
        let parent_name = ctx.name(self.grammar);
        let mut builder = self
            .diag
            .report(DiagnosticKind::InvalidGrammarFieldChildKind, span)
            .detail(format!(
                "{value} is not valid for grammar field `{}` of `{parent_name}`",
                name
            ))
            .related_to(field.span, format!("grammar field `{name}`"))
            .related_to(ctx.span(), format!("`{parent_name}` starts here"));
        if let Some(hint) = hint {
            builder = builder.hint(hint);
        }
        builder.emit();
    }

    /// Hint for the invalid-field-value diagnostic: the named kinds a grammar field accepts, or — for
    /// literal-only fields — a concrete `field: "token"` example.
    fn field_value_hint(
        &self,
        parent_id: NodeKindId,
        field_id: NodeFieldId,
        field_name: &str,
    ) -> Option<String> {
        let types = self.grammar.valid_field_types(parent_id, field_id);
        let named = self.concrete_kinds(types.iter().copied());
        let mut literals = types
            .iter()
            .filter(|&&id| self.grammar.is_anonymous_node(id))
            .map(|&id| {
                self.grammar
                    .node_kind(id)
                    .expect("retained kind has a name")
            })
            .collect::<Vec<_>>();
        literals.sort_unstable();

        if named.is_empty() {
            let example = literals.first()?;
            return Some(format!(
                "`{0}` accepts only literal tokens. Write `{0}: {1:?}`",
                field_name, example
            ));
        }
        let values = named
            .iter()
            .map(|name| name.to_string())
            .chain(literals.iter().map(|literal| format!("{literal:?}")))
            .collect::<Vec<_>>();
        let values = values.iter().map(String::as_str).collect::<Vec<_>>();
        Some(format!(
            "`{}` accepts: {}",
            field_name,
            format_list(&values, 8)
        ))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct FieldRef {
    id: NodeFieldId,
    span: Span,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct ParentNode {
    id: NodeKindId,
    /// Source the parent node lives in. May differ from the source currently
    /// being walked once validation crosses a reference into another workspace
    /// file, so this span must be reported against its own source.
    span: Span,
}

impl ParentNode {
    fn id(self) -> NodeKindId {
        self.id
    }

    fn span(self) -> Span {
        self.span
    }

    fn name(self, grammar: &Grammar) -> &str {
        grammar
            .node_kind(self.id)
            .expect("validated parent node must have a name")
    }
}
