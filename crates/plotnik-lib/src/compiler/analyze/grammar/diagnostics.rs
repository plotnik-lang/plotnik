use crate::compiler::diagnostics::diagnostics::{DiagnosticKind, Span};
use crate::core::{NodeFieldId, NodeKindId};

use super::admissibility::{FieldRef, ParentNodeCtx};
use super::link::GrammarLinker;
use super::utils::find_similar;

impl<'a, 'q> GrammarLinker<'a, 'q> {
    pub(super) fn emit_field_not_on_node(
        &mut self,
        span: Span,
        field_name: &str,
        ctx: &ParentNodeCtx,
    ) {
        let valid_fields = self.grammar.fields_for_node_kind(ctx.parent_id);
        let parent_name = self
            .grammar
            .node_kind(ctx.parent_id)
            .expect("validated parent_id must have a name");

        let mut builder = self
            .diag
            .report(span.source, DiagnosticKind::FieldNotOnNodeKind, span.range)
            .detail(field_name)
            .related_to(
                ctx.parent_source,
                ctx.parent_range,
                format!("on `{}`", parent_name),
            );

        if valid_fields.is_empty() {
            builder = builder.hint(format!("`{}` has no fields", parent_name));
        } else {
            let max_dist = (field_name.len() / 3).clamp(2, 4);
            if let Some(similar) = find_similar(field_name, &valid_fields, max_dist) {
                builder = builder.fix(format!("did you mean `{}`?", similar), similar);
            }
            builder = builder.hint(format!(
                "valid fields for `{}`: {}",
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
        ctx: &ParentNodeCtx,
    ) {
        let child_name = self
            .grammar
            .node_kind(child_id)
            .expect("resolved child must have a name")
            .to_string();
        let parent_name = self
            .grammar
            .node_kind(ctx.parent_id)
            .expect("validated parent_id must have a name")
            .to_string();
        let hint = self.child_hint(ctx.parent_id, &parent_name);

        self.diag
            .report(span.source, DiagnosticKind::InvalidChildType, span.range)
            .detail(child_name)
            .related_to(
                ctx.parent_source,
                ctx.parent_range,
                format!("on `{}`", parent_name),
            )
            .hint(hint)
            .emit();
    }

    pub(super) fn emit_child_under_leaf_token(&mut self, span: Span, ctx: &ParentNodeCtx) {
        let parent_name = self
            .grammar
            .node_kind(ctx.parent_id)
            .expect("validated parent_id must have a name")
            .to_string();

        self.diag
            .report(span.source, DiagnosticKind::ChildUnderLeafToken, span.range)
            .detail(&parent_name)
            .related_to(
                ctx.parent_source,
                ctx.parent_range,
                format!("`{}`", parent_name),
            )
            .hint(format!(
                "a leaf token's content is its text — match it directly `({0})` or by value `({0} == \"foo\")`",
                parent_name
            ))
            .emit();
    }

    /// Hint for the inadmissible-child diagnostic: list valid unlabeled children, or — when a
    /// node's only children are field values — surface those as fields so users don't write ghost
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

        let fields = self.grammar.fields_for_node_kind(parent_id);
        if fields.is_empty() {
            return format!("`{}` has no named children", parent_name);
        }
        let rendered = fields
            .iter()
            .map(|field| self.render_field(parent_id, field))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "`{}` has no unlabeled children — its children are fields: {}",
            parent_name, rendered
        )
    }

    /// Render a field as `name: (kind)` using its first valid kind, for child/field hints.
    fn render_field(&self, parent_id: NodeKindId, field_name: &str) -> String {
        let type_name = self
            .grammar
            .resolve_field(field_name)
            .map(|field_id| self.grammar.valid_field_types(parent_id, field_id))
            .unwrap_or(&[])
            .iter()
            .find_map(|&id| self.grammar.node_kind(id))
            .unwrap_or("_");
        format!("`{}: ({})`", field_name, type_name)
    }

    pub(super) fn emit_invalid_field_value(
        &mut self,
        span: Span,
        message: String,
        ctx: &ParentNodeCtx,
        field: &FieldRef,
    ) {
        let hint = self.field_value_hint(ctx.parent_id, field.id, field.name);
        self.diag
            .report(
                span.source,
                DiagnosticKind::InvalidFieldChildType,
                span.range,
            )
            .detail(message)
            .related_to(
                field.span.source,
                field.span.range,
                format!("field `{}`", field.name),
            )
            .hint(hint)
            .emit();
    }

    /// Hint for the invalid-field-value diagnostic: the named kinds a field accepts, or — for
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
                "`{0}` accepts only literal tokens — write `{0}: \"{1}\"`",
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
        format!(
            "{}, ... ({} more)",
            shown.join(", "),
            items.len() - max_items
        )
    }
}
