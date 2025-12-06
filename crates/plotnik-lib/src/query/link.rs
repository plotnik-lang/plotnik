//! Link pass: resolve node types and fields against tree-sitter grammar.
//!
//! Three-phase approach:
//! 1. Collect and resolve all node type names (NamedNode, AnonymousNode)
//! 2. Collect and resolve all field names (FieldExpr, NegatedField)
//! 3. Validate structural constraints (field on node type, child type for field)

use plotnik_langs::{Lang, NodeTypeId};

use crate::diagnostics::DiagnosticKind;
use crate::parser::ast::{self, Expr, NamedNode};
use crate::parser::cst::{SyntaxKind, SyntaxToken};

use super::Query;

/// Simple edit distance for fuzzy matching (Levenshtein).
fn edit_distance(a: &str, b: &str) -> usize {
    let a_len = a.len();
    let b_len = b.len();

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
            self.validate_expr_structure(&body, None, lang);
        }
    }

    fn validate_expr_structure(
        &mut self,
        expr: &Expr,
        parent_type_id: Option<NodeTypeId>,
        lang: &Lang,
    ) {
        match expr {
            Expr::NamedNode(node) => {
                let current_type_id = self.get_node_type_id(node);
                for child in node.children() {
                    self.validate_expr_structure(&child, current_type_id, lang);
                }
                for child in node.as_cst().children() {
                    if let Some(neg) = ast::NegatedField::cast(child) {
                        self.validate_negated_field(&neg, current_type_id, lang);
                    }
                }
            }
            Expr::FieldExpr(f) => {
                self.validate_field(f, parent_type_id, lang);
                let Some(value) = f.value() else { return };
                self.validate_expr_structure(&value, parent_type_id, lang);
            }
            Expr::AltExpr(alt) => {
                for branch in alt.branches() {
                    let Some(body) = branch.body() else { continue };
                    self.validate_expr_structure(&body, parent_type_id, lang);
                }
            }
            Expr::SeqExpr(seq) => {
                for child in seq.children() {
                    self.validate_expr_structure(&child, parent_type_id, lang);
                }
            }
            Expr::CapturedExpr(cap) => {
                let Some(inner) = cap.inner() else { return };
                self.validate_expr_structure(&inner, parent_type_id, lang);
            }
            Expr::QuantifiedExpr(q) => {
                let Some(inner) = q.inner() else { return };
                self.validate_expr_structure(&inner, parent_type_id, lang);
            }
            Expr::AnonymousNode(_) | Expr::Ref(_) => {}
        }
    }

    fn validate_field(
        &mut self,
        field: &ast::FieldExpr,
        parent_type_id: Option<NodeTypeId>,
        lang: &Lang,
    ) {
        let Some(name_token) = field.name() else {
            return;
        };
        let field_name = name_token.text();

        let Some(field_id) = self.node_field_ids.get(field_name).copied().flatten() else {
            return;
        };

        let Some(parent_id) = parent_type_id else {
            return;
        };
        if !lang.has_field(parent_id, field_id) {
            self.emit_field_not_on_node(name_token.text_range(), field_name, parent_id, lang);
            return;
        }

        let Some(value) = field.value() else {
            return;
        };
        let Some(child_id) = self.get_expr_type_id(&value) else {
            return;
        };
        if lang.is_valid_field_type(parent_id, field_id, child_id) {
            return;
        }
        let child_name = self.get_expr_type_name(&value).unwrap_or("(unknown)");
        let valid_types = lang.valid_field_types(parent_id, field_id);
        let valid_names: Vec<&str> = valid_types
            .iter()
            .filter_map(|&id| lang.node_type_name(id))
            .collect();

        let mut builder = self
            .link_diagnostics
            .report(DiagnosticKind::InvalidFieldChildType, value.text_range())
            .message(child_name);

        if !valid_names.is_empty() {
            builder = builder.hint(format!(
                "valid types for `{}`: {}",
                field_name,
                format_list(&valid_names, 5)
            ));
        }
        builder.emit();
    }

    fn validate_negated_field(
        &mut self,
        neg: &ast::NegatedField,
        parent_type_id: Option<NodeTypeId>,
        lang: &Lang,
    ) {
        let Some(name_token) = neg.name() else {
            return;
        };
        let field_name = name_token.text();

        let Some(field_id) = self.node_field_ids.get(field_name).copied().flatten() else {
            return;
        };

        let Some(parent_id) = parent_type_id else {
            return;
        };
        if lang.has_field(parent_id, field_id) {
            return;
        }
        self.emit_field_not_on_node(name_token.text_range(), field_name, parent_id, lang);
    }

    fn emit_field_not_on_node(
        &mut self,
        range: rowan::TextRange,
        field_name: &str,
        parent_id: NodeTypeId,
        lang: &Lang,
    ) {
        let valid_fields = lang.fields_for_node_type(parent_id);
        let parent_name = lang.node_type_name(parent_id).unwrap_or("(unknown)");

        let mut builder = self
            .link_diagnostics
            .report(DiagnosticKind::FieldNotOnNodeType, range)
            .message(field_name);

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

    fn get_node_type_id(&self, node: &NamedNode) -> Option<NodeTypeId> {
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
        self.node_type_ids.get(type_name).copied().flatten()
    }

    fn get_expr_type_id(&self, expr: &Expr) -> Option<NodeTypeId> {
        match expr {
            Expr::NamedNode(node) => self.get_node_type_id(node),
            Expr::AnonymousNode(anon) => {
                if anon.is_any() {
                    return None;
                }
                let value_token = anon.value()?;
                let value = &self.source[text_range_to_usize(value_token.text_range())];
                self.node_type_ids.get(value).copied().flatten()
            }
            Expr::CapturedExpr(cap) => self.get_expr_type_id(&cap.inner()?),
            Expr::QuantifiedExpr(q) => self.get_expr_type_id(&q.inner()?),
            _ => None,
        }
    }

    fn get_expr_type_name(&self, expr: &Expr) -> Option<&'a str> {
        match expr {
            Expr::NamedNode(node) => {
                if node.is_any() {
                    return None;
                }
                let type_token = node.node_type()?;
                Some(&self.source[text_range_to_usize(type_token.text_range())])
            }
            Expr::AnonymousNode(anon) => {
                if anon.is_any() {
                    return None;
                }
                let value_token = anon.value()?;
                Some(&self.source[text_range_to_usize(value_token.text_range())])
            }
            Expr::CapturedExpr(cap) => self.get_expr_type_name(&cap.inner()?),
            Expr::QuantifiedExpr(q) => self.get_expr_type_name(&q.inner()?),
            _ => None,
        }
    }
}

fn text_range_to_usize(range: rowan::TextRange) -> std::ops::Range<usize> {
    let start: usize = range.start().into();
    let end: usize = range.end().into();
    start..end
}
