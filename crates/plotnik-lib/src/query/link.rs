//! Link pass: resolve node types and fields against tree-sitter grammar.
//!
//! Three-phase approach:
//! 1. Collect and resolve all node type names (NamedNode, AnonymousNode)
//! 2. Collect and resolve all field names (FieldExpr, NegatedField)
//! 3. Validate structural constraints (field on node type, child type for field)

use plotnik_langs::{Lang, NodeTypeId};

use crate::diagnostics::DiagnosticKind;
use crate::parser::ast::{self, Expr, NamedNode};
use crate::parser::cst::SyntaxKind;

use super::Query;

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
        // Skip ERROR and MISSING - they're built-in tree-sitter concepts
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
            self.link_diagnostics
                .report(DiagnosticKind::UnknownNodeType, type_token.text_range())
                .message(type_name)
                .emit();
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
                self.resolve_field_expr(f, lang);
                let Some(value) = f.value() else { return };
                self.collect_fields(&value, lang);
            }
            Expr::AnonymousNode(_) | Expr::Ref(_) => {}
        }

        // Also check NegatedField (predicate, not in Expr enum)
        if let Some(node) = ast::NamedNode::cast(expr.as_cst().clone()) {
            for child in node.as_cst().children() {
                if let Some(neg) = ast::NegatedField::cast(child) {
                    self.resolve_negated_field(&neg, lang);
                }
            }
        }
    }

    fn resolve_field_expr(&mut self, field: &ast::FieldExpr, lang: &Lang) {
        let Some(name_token) = field.name() else {
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
        if resolved.is_none() {
            self.link_diagnostics
                .report(DiagnosticKind::UnknownField, name_token.text_range())
                .message(field_name)
                .emit();
        }
    }

    fn resolve_negated_field(&mut self, neg: &ast::NegatedField, lang: &Lang) {
        let Some(name_token) = neg.name() else {
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
        if resolved.is_none() {
            self.link_diagnostics
                .report(DiagnosticKind::UnknownField, name_token.text_range())
                .message(field_name)
                .emit();
        }
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
                // Also validate NegatedField predicates
                for child in node.as_cst().children() {
                    if let Some(neg) = ast::NegatedField::cast(child) {
                        self.validate_negated_field(&neg, current_type_id, lang);
                    }
                }
            }
            Expr::FieldExpr(f) => {
                self.validate_field(f, parent_type_id, lang);
                let Some(value) = f.value() else { return };
                // Field children don't inherit parent_type_id - they're the field value
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

        // Get field ID - skip if unresolved (already reported)
        let Some(field_id) = self.node_field_ids.get(field_name).copied().flatten() else {
            return;
        };

        // Check if field exists on parent node type
        let Some(parent_id) = parent_type_id else {
            return;
        };
        if !lang.has_field(parent_id, field_id) {
            self.link_diagnostics
                .report(DiagnosticKind::FieldNotOnNodeType, name_token.text_range())
                .message(field_name)
                .emit();
            return;
        }

        // Check if child type is valid for this field
        let Some(value) = field.value() else {
            return;
        };
        let child_type_id = self.get_expr_type_id(&value);
        let Some(child_id) = child_type_id else {
            return;
        };
        if !lang.is_valid_field_type(parent_id, field_id, child_id) {
            let child_name = self.get_expr_type_name(&value).unwrap_or("(unknown)");
            self.link_diagnostics
                .report(DiagnosticKind::InvalidFieldChildType, value.text_range())
                .message(child_name)
                .emit();
        }
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
        if !lang.has_field(parent_id, field_id) {
            self.link_diagnostics
                .report(DiagnosticKind::FieldNotOnNodeType, name_token.text_range())
                .message(field_name)
                .emit();
        }
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
