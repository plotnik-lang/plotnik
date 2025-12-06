//! Link pass: resolve node types and fields against tree-sitter grammar.
//!
//! Three-phase approach:
//! 1. Collect and resolve all node type names (NamedNode, AnonymousNode)
//! 2. Collect and resolve all field names (FieldExpr, NegatedField)
//! 3. Validate structural constraints (field on node type, child type for field)

use indexmap::IndexSet;
use plotnik_langs::{Lang, NodeFieldId, NodeTypeId};
use rowan::TextRange;

use crate::diagnostics::DiagnosticKind;
use crate::parser::ast::{self, Expr, NamedNode};
use crate::parser::cst::{SyntaxKind, SyntaxToken};

use super::Query;

/// Simple edit distance for fuzzy matching (Levenshtein).
fn edit_distance(a: &str, b: &str) -> usize {
    let a_len = a.chars().count();
    let b_len = b.chars().count();

    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }

    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr = vec![0; b_len + 1];

    for (i, ca) in a.chars().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.chars().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b_len]
}

/// Find the best match from candidates within a reasonable edit distance.
fn find_similar<'a>(name: &str, candidates: &[&'a str], max_distance: usize) -> Option<&'a str> {
    candidates
        .iter()
        .map(|&c| (c, edit_distance(name, c)))
        .filter(|(_, d)| *d <= max_distance)
        .min_by_key(|(_, d)| *d)
        .map(|(c, _)| c)
}

/// Check if `child` is a subtype of `supertype`, recursively handling nested supertypes.
fn is_subtype_of(lang: &Lang, child: NodeTypeId, supertype: NodeTypeId) -> bool {
    let subtypes = lang.subtypes(supertype);
    for &subtype in subtypes {
        if subtype == child {
            return true;
        }
        if lang.is_supertype(subtype) && is_subtype_of(lang, child, subtype) {
            return true;
        }
    }
    false
}

/// Check if `child` is a valid non-field child of `parent`, expanding supertypes.
fn is_valid_child_expanded(lang: &Lang, parent: NodeTypeId, child: NodeTypeId) -> bool {
    let valid_types = lang.valid_child_types(parent);
    for &allowed in valid_types {
        if allowed == child {
            return true;
        }
        if lang.is_supertype(allowed) && is_subtype_of(lang, child, allowed) {
            return true;
        }
    }
    false
}

/// Check if `child` is a valid field value type, expanding supertypes.
fn is_valid_field_type_expanded(
    lang: &Lang,
    parent: NodeTypeId,
    field: NodeFieldId,
    child: NodeTypeId,
) -> bool {
    if lang.is_valid_field_type(parent, field, child) {
        return true;
    }
    let valid_types = lang.valid_field_types(parent, field);
    for &allowed in valid_types {
        if lang.is_supertype(allowed) && is_subtype_of(lang, child, allowed) {
            return true;
        }
    }
    false
}

/// Format a list of items for display, truncating if too long.
fn format_list(items: &[&str], max_items: usize) -> String {
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

/// Context for validating child types.
#[derive(Clone, Copy)]
struct ValidationContext<'a> {
    /// The parent node type being validated against.
    parent_id: NodeTypeId,
    /// The parent node's name for error messages.
    parent_name: &'a str,
    /// The parent node type token range for related_to.
    parent_range: TextRange,
    /// If validating a field value, the field info.
    field: Option<FieldContext<'a>>,
}

#[derive(Clone, Copy)]
struct FieldContext<'a> {
    name: &'a str,
    id: NodeFieldId,
    range: TextRange,
}

impl<'a> Query<'a> {
    /// Link query against a language grammar.
    ///
    /// Resolves node types and fields, validates structural constraints.
    pub fn link(&mut self, lang: &Lang) {
        self.resolve_node_types(lang);
        self.resolve_fields(lang);
        self.validate_structure(lang);
    }

    fn resolve_node_types(&mut self, lang: &Lang) {
        let defs: Vec<_> = self.ast.defs().collect();
        for def in defs {
            let Some(body) = def.body() else { continue };
            self.collect_node_types(&body, lang);
        }
    }

    fn collect_node_types(&mut self, expr: &Expr, lang: &Lang) {
        match expr {
            Expr::NamedNode(node) => {
                self.resolve_named_node(node, lang);
                for child in node.children() {
                    self.collect_node_types(&child, lang);
                }
            }
            Expr::AnonymousNode(anon) => {
                if anon.is_any() {
                    return;
                }
                let Some(value_token) = anon.value() else {
                    return;
                };
                let value = value_token.text();
                if self.node_type_ids.contains_key(value) {
                    return;
                }
                let resolved = lang.resolve_anonymous_node(value);
                self.node_type_ids.insert(
                    &self.source[text_range_to_usize(value_token.text_range())],
                    resolved,
                );
                if resolved.is_none() {
                    self.link_diagnostics
                        .report(DiagnosticKind::UnknownNodeType, value_token.text_range())
                        .message(value)
                        .emit();
                }
            }
            Expr::AltExpr(alt) => {
                for branch in alt.branches() {
                    let Some(body) = branch.body() else { continue };
                    self.collect_node_types(&body, lang);
                }
            }
            Expr::SeqExpr(seq) => {
                for child in seq.children() {
                    self.collect_node_types(&child, lang);
                }
            }
            Expr::CapturedExpr(cap) => {
                let Some(inner) = cap.inner() else { return };
                self.collect_node_types(&inner, lang);
            }
            Expr::QuantifiedExpr(q) => {
                let Some(inner) = q.inner() else { return };
                self.collect_node_types(&inner, lang);
            }
            Expr::FieldExpr(f) => {
                let Some(value) = f.value() else { return };
                self.collect_node_types(&value, lang);
            }
            Expr::Ref(_) => {}
        }
    }

    fn resolve_named_node(&mut self, node: &NamedNode, lang: &Lang) {
        if node.is_any() {
            return;
        }
        let Some(type_token) = node.node_type() else {
            return;
        };
        if matches!(
            type_token.kind(),
            SyntaxKind::KwError | SyntaxKind::KwMissing
        ) {
            return;
        }
        let type_name = type_token.text();
        if self.node_type_ids.contains_key(type_name) {
            return;
        }
        let resolved = lang.resolve_named_node(type_name);
        self.node_type_ids.insert(
            &self.source[text_range_to_usize(type_token.text_range())],
            resolved,
        );
        if resolved.is_none() {
            let all_types = lang.all_named_node_kinds();
            let max_dist = (type_name.len() / 3).clamp(2, 4);
            let suggestion = find_similar(type_name, &all_types, max_dist);

            let mut builder = self
                .link_diagnostics
                .report(DiagnosticKind::UnknownNodeType, type_token.text_range())
                .message(type_name);

            if let Some(similar) = suggestion {
                builder = builder.hint(format!("did you mean `{}`?", similar));
            }
            builder.emit();
        }
    }

    fn resolve_fields(&mut self, lang: &Lang) {
        let defs: Vec<_> = self.ast.defs().collect();
        for def in defs {
            let Some(body) = def.body() else { continue };
            self.collect_fields(&body, lang);
        }
    }

    fn collect_fields(&mut self, expr: &Expr, lang: &Lang) {
        match expr {
            Expr::NamedNode(node) => {
                for child in node.children() {
                    self.collect_fields(&child, lang);
                }
                for child in node.as_cst().children() {
                    if let Some(neg) = ast::NegatedField::cast(child) {
                        self.resolve_field_by_token(neg.name(), lang);
                    }
                }
            }
            Expr::AltExpr(alt) => {
                for branch in alt.branches() {
                    let Some(body) = branch.body() else { continue };
                    self.collect_fields(&body, lang);
                }
            }
            Expr::SeqExpr(seq) => {
                for child in seq.children() {
                    self.collect_fields(&child, lang);
                }
            }
            Expr::CapturedExpr(cap) => {
                let Some(inner) = cap.inner() else { return };
                self.collect_fields(&inner, lang);
            }
            Expr::QuantifiedExpr(q) => {
                let Some(inner) = q.inner() else { return };
                self.collect_fields(&inner, lang);
            }
            Expr::FieldExpr(f) => {
                self.resolve_field_by_token(f.name(), lang);
                let Some(value) = f.value() else { return };
                self.collect_fields(&value, lang);
            }
            Expr::AnonymousNode(_) | Expr::Ref(_) => {}
        }
    }

    fn resolve_field_by_token(&mut self, name_token: Option<SyntaxToken>, lang: &Lang) {
        let Some(name_token) = name_token else {
            return;
        };
        let field_name = name_token.text();
        if self.node_field_ids.contains_key(field_name) {
            return;
        }
        let resolved = lang.resolve_field(field_name);
        self.node_field_ids.insert(
            &self.source[text_range_to_usize(name_token.text_range())],
            resolved,
        );
        if resolved.is_some() {
            return;
        }
        let all_fields = lang.all_field_names();
        let max_dist = (field_name.len() / 3).clamp(2, 4);
        let suggestion = find_similar(field_name, &all_fields, max_dist);

        let mut builder = self
            .link_diagnostics
            .report(DiagnosticKind::UnknownField, name_token.text_range())
            .message(field_name);

        if let Some(similar) = suggestion {
            builder = builder.hint(format!("did you mean `{}`?", similar));
        }
        builder.emit();
    }

    fn validate_structure(&mut self, lang: &Lang) {
        let defs: Vec<_> = self.ast.defs().collect();
        for def in defs {
            let Some(body) = def.body() else { continue };
            let mut visited = IndexSet::new();
            self.validate_expr_structure(&body, None, lang, &mut visited);
        }
    }

    fn validate_expr_structure(
        &mut self,
        expr: &Expr,
        ctx: Option<ValidationContext<'a>>,
        lang: &Lang,
        visited: &mut IndexSet<String>,
    ) {
        match expr {
            Expr::NamedNode(node) => {
                // Validate this node against the context (if any)
                if let Some(ref ctx) = ctx {
                    self.validate_terminal_type(expr, ctx, lang, visited);
                }

                // Set up context for children
                let child_ctx = self.make_node_context(node, lang);

                for child in node.children() {
                    match &child {
                        Expr::FieldExpr(f) => {
                            // Fields get special handling
                            self.validate_field_expr(f, child_ctx.as_ref(), lang, visited);
                        }
                        _ => {
                            // Non-field children: validate as non-field children
                            if let Some(ctx) = child_ctx {
                                self.validate_non_field_children(&child, &ctx, lang, visited);
                            }
                            self.validate_expr_structure(&child, child_ctx, lang, visited);
                        }
                    }
                }

                // Handle negated fields
                if let Some(ctx) = child_ctx {
                    for child in node.as_cst().children() {
                        if let Some(neg) = ast::NegatedField::cast(child) {
                            self.validate_negated_field(&neg, &ctx, lang);
                        }
                    }
                }
            }
            Expr::AnonymousNode(_) => {
                // Validate this anonymous node against the context (if any)
                if let Some(ref ctx) = ctx {
                    self.validate_terminal_type(expr, ctx, lang, visited);
                }
            }
            Expr::FieldExpr(f) => {
                // Should be handled by parent NamedNode, but handle gracefully
                self.validate_field_expr(f, ctx.as_ref(), lang, visited);
            }
            Expr::AltExpr(alt) => {
                for branch in alt.branches() {
                    let Some(body) = branch.body() else { continue };
                    self.validate_expr_structure(&body, ctx, lang, visited);
                }
            }
            Expr::SeqExpr(seq) => {
                for child in seq.children() {
                    self.validate_expr_structure(&child, ctx, lang, visited);
                }
            }
            Expr::CapturedExpr(cap) => {
                let Some(inner) = cap.inner() else { return };
                self.validate_expr_structure(&inner, ctx, lang, visited);
            }
            Expr::QuantifiedExpr(q) => {
                let Some(inner) = q.inner() else { return };
                self.validate_expr_structure(&inner, ctx, lang, visited);
            }
            Expr::Ref(r) => {
                let Some(name_token) = r.name() else { return };
                let name = name_token.text();
                if !visited.insert(name.to_string()) {
                    return;
                }
                let Some(body) = self.symbol_table.get(name).cloned() else {
                    visited.swap_remove(name);
                    return;
                };
                self.validate_expr_structure(&body, ctx, lang, visited);
                visited.swap_remove(name);
            }
        }
    }

    /// Create validation context for a named node's children.
    fn make_node_context(&self, node: &NamedNode, lang: &Lang) -> Option<ValidationContext<'a>> {
        if node.is_any() {
            return None;
        }
        let type_token = node.node_type()?;
        if matches!(
            type_token.kind(),
            SyntaxKind::KwError | SyntaxKind::KwMissing
        ) {
            return None;
        }
        let type_name = type_token.text();
        let parent_id = self.node_type_ids.get(type_name).copied().flatten()?;
        let parent_name = lang.node_type_name(parent_id)?;
        Some(ValidationContext {
            parent_id,
            parent_name,
            parent_range: type_token.text_range(),
            field: None,
        })
    }

    /// Validate a field expression.
    fn validate_field_expr(
        &mut self,
        field: &ast::FieldExpr,
        ctx: Option<&ValidationContext<'a>>,
        lang: &Lang,
        visited: &mut IndexSet<String>,
    ) {
        let Some(name_token) = field.name() else {
            return;
        };
        let field_name = name_token.text();

        let Some(field_id) = self.node_field_ids.get(field_name).copied().flatten() else {
            return;
        };

        let Some(ctx) = ctx else {
            return;
        };

        // Check field exists on parent
        if !lang.has_field(ctx.parent_id, field_id) {
            self.emit_field_not_on_node(
                name_token.text_range(),
                field_name,
                ctx.parent_id,
                ctx.parent_range,
                lang,
            );
            return;
        }

        let Some(value) = field.value() else {
            return;
        };

        // Create field context for validating the value
        let field_ctx = ValidationContext {
            parent_id: ctx.parent_id,
            parent_name: ctx.parent_name,
            parent_range: ctx.parent_range,
            field: Some(FieldContext {
                name: &self.source[text_range_to_usize(name_token.text_range())],
                id: field_id,
                range: name_token.text_range(),
            }),
        };

        // Validate field value - this will traverse through alt/seq/quantifier/capture
        // and validate each terminal type against the field requirements
        self.validate_expr_structure(&value, Some(field_ctx), lang, visited);
    }

    /// Validate non-field children. Called for direct children of a NamedNode that aren't fields.
    fn validate_non_field_children(
        &mut self,
        expr: &Expr,
        ctx: &ValidationContext<'a>,
        lang: &Lang,
        visited: &mut IndexSet<String>,
    ) {
        // Collect all terminal types from this expression (follows refs)
        let terminals = self.collect_terminal_types(expr, visited);

        // Check if parent allows any non-field children
        let valid_types = lang.valid_child_types(ctx.parent_id);
        let parent_only_fields = valid_types.is_empty();

        for (child_id, child_name, child_range) in terminals {
            if parent_only_fields {
                self.link_diagnostics
                    .report(DiagnosticKind::InvalidChildType, child_range)
                    .message(child_name)
                    .related_to(
                        format!("`{}` only accepts children via fields", ctx.parent_name),
                        ctx.parent_range,
                    )
                    .emit();
                continue;
            }

            if is_valid_child_expanded(lang, ctx.parent_id, child_id) {
                continue;
            }

            let valid_names: Vec<&str> = valid_types
                .iter()
                .filter_map(|&id| lang.node_type_name(id))
                .collect();

            let mut builder = self
                .link_diagnostics
                .report(DiagnosticKind::InvalidChildType, child_range)
                .message(child_name)
                .related_to(format!("inside `{}`", ctx.parent_name), ctx.parent_range);

            if !valid_names.is_empty() {
                builder = builder.hint(format!(
                    "valid children for `{}`: {}",
                    ctx.parent_name,
                    format_list(&valid_names, 5)
                ));
            }
            builder.emit();
        }
    }

    /// Validate a terminal type (NamedNode or AnonymousNode) against the context.
    fn validate_terminal_type(
        &mut self,
        expr: &Expr,
        ctx: &ValidationContext<'a>,
        lang: &Lang,
        visited: &mut IndexSet<String>,
    ) {
        // Handle refs by following them
        if let Expr::Ref(r) = expr {
            let Some(name_token) = r.name() else { return };
            let name = name_token.text();
            if !visited.insert(name.to_string()) {
                return;
            }
            let Some(body) = self.symbol_table.get(name).cloned() else {
                visited.swap_remove(name);
                return;
            };
            self.validate_terminal_type(&body, ctx, lang, visited);
            visited.swap_remove(name);
            return;
        }

        let Some((child_id, child_name, child_range)) = self.get_terminal_type_info(expr) else {
            return;
        };

        if let Some(ref field) = ctx.field {
            // Validating a field value
            if is_valid_field_type_expanded(lang, ctx.parent_id, field.id, child_id) {
                return;
            }

            let valid_types = lang.valid_field_types(ctx.parent_id, field.id);
            let valid_names: Vec<&str> = valid_types
                .iter()
                .filter_map(|&id| lang.node_type_name(id))
                .collect();

            let mut builder = self
                .link_diagnostics
                .report(DiagnosticKind::InvalidFieldChildType, child_range)
                .message(child_name)
                .related_to(
                    format!("field `{}` on `{}`", field.name, ctx.parent_name),
                    field.range,
                );

            if !valid_names.is_empty() {
                builder = builder.hint(format!(
                    "valid types for `{}`: {}",
                    field.name,
                    format_list(&valid_names, 5)
                ));
            }
            builder.emit();
        }
        // Non-field children are validated by validate_non_field_children
    }

    /// Collect all terminal types from an expression (traverses through Alt/Seq/Capture/Quantifier/Ref).
    fn collect_terminal_types(
        &self,
        expr: &Expr,
        visited: &mut IndexSet<String>,
    ) -> Vec<(NodeTypeId, &'a str, TextRange)> {
        let mut result = Vec::new();
        self.collect_terminal_types_impl(expr, &mut result, visited);
        result
    }

    fn collect_terminal_types_impl(
        &self,
        expr: &Expr,
        result: &mut Vec<(NodeTypeId, &'a str, TextRange)>,
        visited: &mut IndexSet<String>,
    ) {
        match expr {
            Expr::NamedNode(_) | Expr::AnonymousNode(_) => {
                if let Some(info) = self.get_terminal_type_info(expr) {
                    result.push(info);
                }
            }
            Expr::AltExpr(alt) => {
                for branch in alt.branches() {
                    if let Some(body) = branch.body() {
                        self.collect_terminal_types_impl(&body, result, visited);
                    }
                }
            }
            Expr::SeqExpr(seq) => {
                for child in seq.children() {
                    self.collect_terminal_types_impl(&child, result, visited);
                }
            }
            Expr::CapturedExpr(cap) => {
                if let Some(inner) = cap.inner() {
                    self.collect_terminal_types_impl(&inner, result, visited);
                }
            }
            Expr::QuantifiedExpr(q) => {
                if let Some(inner) = q.inner() {
                    self.collect_terminal_types_impl(&inner, result, visited);
                }
            }
            Expr::Ref(r) => {
                let Some(name_token) = r.name() else { return };
                let name = name_token.text();
                if !visited.insert(name.to_string()) {
                    return;
                }
                let Some(body) = self.symbol_table.get(name) else {
                    visited.swap_remove(name);
                    return;
                };
                self.collect_terminal_types_impl(body, result, visited);
                visited.swap_remove(name);
            }
            Expr::FieldExpr(_) => {
                // Fields are handled separately
            }
        }
    }

    /// Get type info for a terminal expression (NamedNode or AnonymousNode).
    fn get_terminal_type_info(&self, expr: &Expr) -> Option<(NodeTypeId, &'a str, TextRange)> {
        match expr {
            Expr::NamedNode(node) => {
                if node.is_any() {
                    return None;
                }
                let type_token = node.node_type()?;
                if matches!(
                    type_token.kind(),
                    SyntaxKind::KwError | SyntaxKind::KwMissing
                ) {
                    return None;
                }
                let type_name = type_token.text();
                let type_id = self.node_type_ids.get(type_name).copied().flatten()?;
                let name = &self.source[text_range_to_usize(type_token.text_range())];
                Some((type_id, name, type_token.text_range()))
            }
            Expr::AnonymousNode(anon) => {
                if anon.is_any() {
                    return None;
                }
                let value_token = anon.value()?;
                let value = &self.source[text_range_to_usize(value_token.text_range())];
                let type_id = self.node_type_ids.get(value).copied().flatten()?;
                Some((type_id, value, value_token.text_range()))
            }
            _ => None,
        }
    }

    fn validate_negated_field(
        &mut self,
        neg: &ast::NegatedField,
        ctx: &ValidationContext<'a>,
        lang: &Lang,
    ) {
        let Some(name_token) = neg.name() else {
            return;
        };
        let field_name = name_token.text();

        let Some(field_id) = self.node_field_ids.get(field_name).copied().flatten() else {
            return;
        };

        if lang.has_field(ctx.parent_id, field_id) {
            return;
        }
        self.emit_field_not_on_node(
            name_token.text_range(),
            field_name,
            ctx.parent_id,
            ctx.parent_range,
            lang,
        );
    }

    fn emit_field_not_on_node(
        &mut self,
        range: TextRange,
        field_name: &str,
        parent_id: NodeTypeId,
        parent_range: TextRange,
        lang: &Lang,
    ) {
        let valid_fields = lang.fields_for_node_type(parent_id);
        let parent_name = lang.node_type_name(parent_id).unwrap_or("(unknown)");

        let mut builder = self
            .link_diagnostics
            .report(DiagnosticKind::FieldNotOnNodeType, range)
            .message(field_name)
            .related_to(format!("on `{}`", parent_name), parent_range);

        if valid_fields.is_empty() {
            builder = builder.hint(format!("`{}` has no fields", parent_name));
        } else {
            let max_dist = (field_name.len() / 3).clamp(2, 4);
            if let Some(similar) = find_similar(field_name, &valid_fields, max_dist) {
                builder = builder.hint(format!("did you mean `{}`?", similar));
            }
            builder = builder.hint(format!(
                "valid fields for `{}`: {}",
                parent_name,
                format_list(&valid_fields, 5)
            ));
        }
        builder.emit();
    }
}

fn text_range_to_usize(range: TextRange) -> std::ops::Range<usize> {
    let start: usize = range.start().into();
    let end: usize = range.end().into();
    start..end
}
