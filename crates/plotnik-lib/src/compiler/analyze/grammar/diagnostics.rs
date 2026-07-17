use crate::compiler::diagnostics::report::{DiagnosticKind, Span};
use crate::core::{NodeFieldId, NodeKindId};

use super::bind::GrammarBinder;
use super::check::{FieldRef, ParentNode};
use super::utils::find_similar;

impl<'a, 'q> GrammarBinder<'a, 'q> {
    pub(super) fn emit_field_not_on_node(
        &mut self,
        span: Span,
        field_name: &str,
        ctx: &ParentNode,
    ) {
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

    pub(super) fn emit_invalid_child(
        &mut self,
        span: Span,
        child_id: NodeKindId,
        ctx: &ParentNode,
    ) {
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

    pub(super) fn emit_child_under_leaf_token(&mut self, span: Span, ctx: &ParentNode) {
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

    pub(super) fn emit_invalid_field_value(
        &mut self,
        span: Span,
        value: &str,
        ctx: &ParentNode,
        field: &FieldRef,
    ) {
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
