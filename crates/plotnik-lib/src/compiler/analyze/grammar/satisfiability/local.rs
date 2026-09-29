use std::collections::HashSet;

use super::super::format::{format_kind, format_list};
use super::super::model::{KindConstraint, Model, NodeViolation, PatternId, PatternKind};
use super::AnalysisLimit;
use super::automaton::{ChildAutomaton, ChildMatcher};
use super::engine::SatisfiabilitySolver;
use crate::compiler::analyze::Located;
use crate::compiler::diagnostics::report::{DiagnosticKind, Span};
use crate::compiler::parse::ast::{self, QuantifierKind};
use crate::core::{NodeFieldId, NodeKindId};

use crate::compiler::diagnostics::report::{DiagnosticBuilder, Diagnostics};
use crate::core::grammar::Grammar;

pub(super) struct LocalDiagnostics<'a, 'q> {
    grammar: &'q Grammar,
    diag: &'a mut Diagnostics,
    model: &'q Model,
    solver: &'a mut SatisfiabilitySolver<'q>,
    reported: bool,
    reported_nodes: &'a mut HashSet<PatternId>,
    visited: HashSet<(PatternId, Option<ParentNode>, Option<FieldRef>)>,
}

impl<'q> LocalDiagnostics<'_, 'q> {
    /// Explain simple structural contradictions only after the solver has rejected
    /// the enclosing pattern. These summaries choose wording, never admission.
    pub(super) fn report(
        grammar: &'q Grammar,
        model: &'q Model,
        solver: &mut SatisfiabilitySolver<'q>,
        pattern: PatternId,
        diag: &mut Diagnostics,
        reported_nodes: &mut HashSet<PatternId>,
    ) -> Result<bool, AnalysisLimit> {
        let mut reporter = LocalDiagnostics {
            grammar,
            model,
            solver,
            diag,
            reported: false,
            reported_nodes,
            visited: HashSet::new(),
        };
        reporter.walk(pattern, None, None)?;
        Ok(reporter.reported)
    }

    fn walk(
        &mut self,
        pattern: PatternId,
        parent: Option<ParentNode>,
        field: Option<FieldRef>,
    ) -> Result<(), AnalysisLimit> {
        if !self.visited.insert((pattern, parent, field)) {
            return Ok(());
        }

        match &self.model.pattern(pattern).kind {
            PatternKind::Node(node) => {
                if let Some(parent) = parent {
                    self.child(pattern, node.kind, &parent, field.as_ref())?;
                }
                if self.reported_nodes.contains(&pattern) {
                    self.reported = true;
                    return Ok(());
                }

                let before = self.diag.len();
                let origin = self.model.node_origin(pattern);
                let KindConstraint::Exact(kind) = node.kind else {
                    return Ok(());
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
                    self.walk(pattern, Some(ctx), None)?;
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
                )?;
            }
            PatternKind::Sequence(items) => {
                for pattern in items.iter().filter_map(|item| item.pattern()) {
                    // A group does not carry its outer field into its siblings.
                    self.walk(pattern, parent, None)?;
                }
            }
            PatternKind::Repeat(repeat) if repeat.kind == QuantifierKind::OneOrMore => {
                self.walk(repeat.inner, parent, field)?;
            }
            PatternKind::Reference(definition) => {
                self.walk(self.model.definition(*definition), parent, field)?;
            }
            PatternKind::Leaf(kind) => {
                if let Some(parent) = parent {
                    self.child(pattern, *kind, &parent, field.as_ref())?;
                }
            }
            PatternKind::Alternative(_) | PatternKind::Repeat(_) => {}
        }
        Ok(())
    }

    fn child(
        &mut self,
        pattern: PatternId,
        kind: KindConstraint,
        parent: &ParentNode,
        field: Option<&FieldRef>,
    ) -> Result<(), AnalysisLimit> {
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
                return Ok(());
            }

            let accepted = self.solver.probe(
                parent.id(),
                ChildAutomaton::child(ChildMatcher::new(kind, Some(field.id))),
            )?;
            if !accepted {
                let value = match kind {
                    KindConstraint::Exact(kind) => format!("`{}`", format_kind(self.grammar, kind)),
                    _ => "a named node".to_string(),
                };
                self.emit_invalid_field_value(span, &value, parent, field)?;
            }
            return Ok(());
        }

        if !matches!(origin.node(), ast::Pattern::NamedNodePattern(_))
            || matches!(kind, KindConstraint::Unconstrained)
        {
            return Ok(());
        }

        if self.grammar.is_token(parent.id()) {
            self.emit_child_under_leaf_token(span, parent);
            return Ok(());
        }

        let KindConstraint::Exact(kind) = kind else {
            return Ok(());
        };
        let accepted = self.grammar.is_extra(kind)
            || self
                .child_kinds(parent.id())
                .any(|candidate| self.accepts_kind(candidate, kind));
        if !accepted {
            self.emit_invalid_child(span, kind, parent);
        }
        Ok(())
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
    ) -> Result<(), AnalysisLimit> {
        self.reported = true;
        let name = self
            .grammar
            .field_name(field.id)
            .expect("resolved field has a name");
        let hint = self.field_value_hint(ctx.id(), field.id, name)?;
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
        Ok(())
    }

    /// Hint for the invalid-field-value diagnostic: the named kinds a grammar field accepts, or — for
    /// literal-only fields — a concrete `field: "token"` example.
    fn field_value_hint(
        &mut self,
        parent_id: NodeKindId,
        field_id: NodeFieldId,
        field_name: &str,
    ) -> Result<Option<String>, AnalysisLimit> {
        let types = self.grammar.valid_field_types(parent_id, field_id);
        let mut candidates = Vec::new();
        for &kind in types {
            if self.grammar.is_supertype(kind) {
                candidates.extend(self.grammar.collect_subtypes(kind));
            } else {
                candidates.push(kind);
            }
        }
        candidates.sort_unstable();
        candidates.dedup();
        let mut accepted = Vec::new();
        for kind in candidates {
            if self.solver.probe(
                parent_id,
                ChildAutomaton::child(ChildMatcher::new(
                    KindConstraint::Exact(kind),
                    Some(field_id),
                )),
            )? {
                accepted.push(kind);
            }
        }
        accepted.sort_by_key(|&kind| {
            (
                self.grammar.is_anonymous_node(kind),
                format_kind(self.grammar, kind),
            )
        });
        if accepted.is_empty() {
            return Ok(None);
        }

        if accepted
            .iter()
            .all(|&kind| self.grammar.is_anonymous_node(kind))
        {
            let example = format_kind(self.grammar, accepted[0]);
            return Ok(Some(format!(
                "`{field_name}` accepts only literal tokens. Write `{field_name}: {example}`"
            )));
        }
        let values = accepted
            .into_iter()
            .map(|kind| format_kind(self.grammar, kind))
            .collect::<Vec<_>>();
        let values = values.iter().map(String::as_str).collect::<Vec<_>>();
        Ok(Some(format!(
            "`{}` accepts: {}",
            field_name,
            format_list(&values, 8)
        )))
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
