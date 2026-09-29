use std::collections::HashSet;

use super::model::{KindConstraint, Model, NodeViolation, PatternId, PatternKind};
use crate::compiler::analyze::Located;
use crate::compiler::diagnostics::report::{DiagnosticKind, Span};
use crate::compiler::parse::ast::{self, QuantifierKind};
use crate::core::{NodeFieldId, NodeKindId};

use super::utils::find_similar;
use crate::compiler::diagnostics::report::Diagnostics;
use crate::core::grammar::Grammar;

pub(super) struct StructuralDiagnostics<'a> {
    grammar: &'a Grammar,
    diag: &'a mut Diagnostics,
    model: &'a Model,
    reported: bool,
    reported_nodes: &'a mut HashSet<PatternId>,
    visited: HashSet<(PatternId, Option<ParentNode>)>,
}

impl StructuralDiagnostics<'_> {
    /// Explain simple structural contradictions only after the solver has rejected
    /// the enclosing pattern. These summaries choose wording, never admission.
    pub(super) fn report(
        grammar: &Grammar,
        model: &Model,
        pattern: PatternId,
        diag: &mut Diagnostics,
        reported_nodes: &mut HashSet<PatternId>,
    ) -> bool {
        let mut reporter = StructuralDiagnostics {
            grammar,
            model,
            diag,
            reported: false,
            reported_nodes,
            visited: HashSet::new(),
        };
        reporter.walk(pattern, None);
        reporter.reported
    }

    fn walk(&mut self, pattern: PatternId, parent: Option<ParentNode>) {
        if !self.visited.insert((pattern, parent)) {
            return;
        }

        match &self.model.pattern(pattern).kind {
            PatternKind::Node(node) => {
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
                        self.emit_predicate_on_non_leaf(&origin, &ctx);
                    }
                    Some(NodeViolation::FieldNotOnKind(field)) => {
                        let name = self
                            .grammar
                            .field_name(field)
                            .expect("resolved field has a name");
                        let span = self.negated_span(pattern, name);
                        self.emit_field_not_on_node(span, name, &ctx);
                    }
                    None => {}
                }
                for item in &node.children {
                    if !matches!(
                        self.model.pattern(item.pattern).kind,
                        PatternKind::Field(_, _)
                    ) {
                        self.child(item.pattern, &ctx, None, &mut HashSet::new());
                    }
                    self.walk(item.pattern, Some(ctx));
                }
                for &field in &node.negated_fields {
                    if !self
                        .grammar
                        .field_cardinality(kind, field)
                        .is_some_and(|cardinality| cardinality.is_required())
                    {
                        continue;
                    }

                    let name = self
                        .grammar
                        .field_name(field)
                        .expect("resolved field has a name");
                    let span = self.negated_span(pattern, name);
                    self.emit_negated_required_field(span, name, &ctx);
                }
                if self.diag.len() != before {
                    self.reported_nodes.insert(pattern);
                }
            }
            PatternKind::Field(id, inner) => {
                let Some(parent) = parent else {
                    return;
                };
                let origin = self.model.pattern(pattern).syntax();
                let ast::Pattern::FieldPattern(field) = origin.node() else {
                    unreachable!("resolved field originates in field syntax");
                };
                let token = field.name().expect("resolved field has a name");
                let field = FieldRef {
                    id: *id,
                    name: token.text(),
                    span: origin.span_of(token.text_range()),
                };
                if !self.grammar.has_field(parent.id(), *id) {
                    self.emit_field_not_on_node(field.span, field.name, &parent);
                    return;
                }

                self.child(*inner, &parent, Some(&field), &mut HashSet::new());
                self.walk(*inner, Some(parent));
            }
            PatternKind::Sequence(items) => {
                for item in items {
                    self.walk(item.pattern, parent);
                }
            }
            PatternKind::Repeat(QuantifierKind::OneOrMore, inner) => self.walk(*inner, parent),
            PatternKind::Reference(definition) => {
                self.walk(self.model.definition(*definition), parent)
            }
            PatternKind::Alternative(_) | PatternKind::Repeat(_, _) | PatternKind::Leaf(_) => {}
        }
    }

    fn negated_span(&self, pattern: PatternId, name: &str) -> Span {
        let node = self.model.node_origin(pattern);
        let token = node
            .node()
            .syntax()
            .children()
            .filter_map(ast::NegatedField::cast)
            .filter_map(|field| field.name())
            .find(|token| token.text() == name)
            .expect("resolved negated field has source syntax");
        node.span_of(token.text_range())
    }

    fn child(
        &mut self,
        pattern: PatternId,
        parent: &ParentNode,
        field: Option<&FieldRef>,
        visited: &mut HashSet<PatternId>,
    ) {
        if !visited.insert(pattern) {
            return;
        }

        let kind = match &self.model.pattern(pattern).kind {
            PatternKind::Node(node) => node.kind,
            PatternKind::Leaf(kind) => *kind,
            PatternKind::Sequence(items) => {
                for item in items {
                    self.child(item.pattern, parent, field, visited);
                }
                return;
            }
            PatternKind::Reference(definition) => {
                self.child(self.model.definition(*definition), parent, field, visited);
                return;
            }
            _ => return,
        };
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
            let types = self.grammar.valid_field_types(parent.id(), field.id);
            let accepted = match kind {
                KindConstraint::Exact(kind) => types.iter().any(|&candidate| {
                    candidate == kind || self.grammar.collect_subtypes(candidate).contains(&kind)
                }),
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
        let candidates = self
            .grammar
            .valid_child_types(parent.id())
            .iter()
            .copied()
            .chain(
                self.grammar
                    .field_ids_for_node_kind(parent.id())
                    .iter()
                    .flat_map(|&field| {
                        self.grammar
                            .valid_field_types(parent.id(), field)
                            .iter()
                            .copied()
                    }),
            );
        if !self.grammar.is_extra(kind)
            && !candidates.into_iter().any(|candidate| {
                candidate == kind || self.grammar.collect_subtypes(candidate).contains(&kind)
            })
        {
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
                "move `{text}` to a leaf child of `{name}`, or match a literal token directly"
            ))
            .emit();
        self.reported = true;
    }

    fn emit_negated_required_field(&mut self, span: Span, name: &str, ctx: &ParentNode) {
        let parent_name = ctx.name(self.grammar);
        self.diag
            .report(DiagnosticKind::NegatedRequiredField, span)
            .detail(name)
            .related_to(ctx.span(), format!("on `{parent_name}`"))
            .hint(format!(
                "`-{name}` requires `{name}` to be absent, but every `{parent_name}` has one. Drop `-{name}`"
            ))
            .emit();
        self.reported = true;
    }

    fn emit_field_not_on_node(&mut self, span: Span, field_name: &str, ctx: &ParentNode) {
        self.reported = true;
        let valid_fields = self.grammar.fields_for_node_kind(ctx.id());
        let parent_name = ctx.name(self.grammar);
        let parent_span = ctx.span();

        let mut builder = self
            .diag
            .report(DiagnosticKind::GrammarFieldNotOnNodeKind, span)
            .detail(format!(
                "`{field_name}` is not a grammar field of `{parent_name}`"
            ))
            .related_to(parent_span, format!("`{parent_name}` starts here"));

        if valid_fields.is_empty() {
            builder = builder.hint(format!("`{}` has no grammar fields", parent_name));
        } else {
            if let Some(similar) = find_similar(field_name, &valid_fields) {
                builder = builder.fix(format!("replace with `{similar}`"), similar);
            }
            builder = builder.hint(format!(
                "valid grammar fields for `{}`: {}",
                parent_name,
                format_list(&valid_fields, 5)
            ));
        }
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
                "`{child_name}` cannot be an unlabeled child of `{parent_name}`"
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

    /// Hint for the inadmissible-child diagnostic: list valid unlabeled children, or — when a
    /// node's only children are grammar-field values — surface those constraints so users don't write ghost
    /// bare-child queries.
    fn child_hint(&self, parent_id: NodeKindId, parent_name: &str) -> String {
        let child_types = self.grammar.valid_child_types(parent_id);
        if !child_types.is_empty() {
            let names = child_types
                .iter()
                .filter_map(|&id| self.grammar.node_kind(id))
                .collect::<Vec<_>>();
            return format!(
                "valid children of `{}`: {}",
                parent_name,
                format_list(&names, 8)
            );
        }

        let field_ids = self.grammar.field_ids_for_node_kind(parent_id);
        if field_ids.is_empty() {
            return format!("`{}` has no named children", parent_name);
        }
        let rendered = field_ids
            .iter()
            .map(|&field| self.render_field(parent_id, field))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "`{}` has no unlabeled children. Use its grammar fields: {}",
            parent_name, rendered
        )
    }

    /// Render a grammar field as `name: (kind)` using its first valid kind.
    fn render_field(&self, parent_id: NodeKindId, field_id: NodeFieldId) -> String {
        let field_name = self
            .grammar
            .field_name(field_id)
            .expect("admissible field id must have a name");
        let type_name = self
            .grammar
            .valid_field_types(parent_id, field_id)
            .iter()
            .find_map(|&id| self.grammar.node_kind(id))
            .unwrap_or("_");
        format!("`{}: ({})`", field_name, type_name)
    }

    fn emit_invalid_field_value(
        &mut self,
        span: Span,
        value: &str,
        ctx: &ParentNode,
        field: &FieldRef,
    ) {
        self.reported = true;
        let hint = self.field_value_hint(ctx.id(), field.id, field.name);
        let parent_name = ctx.name(self.grammar);
        self.diag
            .report(DiagnosticKind::InvalidGrammarFieldChildKind, span)
            .detail(format!(
                "{value} is not valid for grammar field `{}` of `{parent_name}`",
                field.name
            ))
            .related_to(field.span, format!("grammar field `{}`", field.name))
            .related_to(ctx.span(), format!("`{parent_name}` starts here"))
            .hint(hint)
            .emit();
    }

    /// Hint for the invalid-field-value diagnostic: the named kinds a grammar field accepts, or — for
    /// literal-only fields — a concrete `field: "token"` example.
    fn field_value_hint(
        &self,
        parent_id: NodeKindId,
        field_id: NodeFieldId,
        field_name: &str,
    ) -> String {
        let types = self.grammar.valid_field_types(parent_id, field_id);
        let named = types
            .iter()
            .filter(|&&id| !self.grammar.is_anonymous_node(id))
            .filter_map(|&id| self.grammar.node_kind(id))
            .collect::<Vec<_>>();

        if named.is_empty() {
            let example = types
                .iter()
                .find_map(|&id| self.grammar.node_kind(id))
                .unwrap_or("…");
            return format!(
                "`{0}` accepts only literal tokens. Write `{0}: \"{1}\"`",
                field_name, example
            );
        }
        format!("`{}` accepts: {}", field_name, format_list(&named, 8))
    }
}

pub(super) fn format_list(items: &[&str], max_items: usize) -> String {
    if items.is_empty() {
        return String::new();
    }
    if items.len() <= max_items {
        items
            .iter()
            .map(|s| format!("`{}`", s))
            .collect::<Vec<_>>()
            .join(", ")
    } else {
        let shown: Vec<_> = items[..max_items]
            .iter()
            .map(|s| format!("`{}`", s))
            .collect();
        format!("{}, … ({} more)", shown.join(", "), items.len() - max_items)
    }
}

struct FieldRef<'a> {
    id: NodeFieldId,
    name: &'a str,
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
