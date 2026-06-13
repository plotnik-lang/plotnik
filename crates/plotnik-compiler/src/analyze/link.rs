//! Link pass: resolve node types and fields against tree-sitter grammar.
//!
//! Two-phase approach:
//! 1. Resolve all symbols (node types and fields) against grammar
//! 2. Validate structural constraints (field on node type, child type for field)

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;
use plotnik_core::grammar::Grammar;
use plotnik_core::{Interner, NodeFieldId, NodeType, NodeTypeId, Symbol};
use rowan::TextRange;

/// Output from the link phase for binary emission.
#[derive(Default)]
pub struct LinkOutput {
    /// Interned named/anonymous node type → NodeTypeId (for binary: StringId → NodeTypeId)
    pub node_type_ids: IndexMap<NodeType<Symbol>, NodeTypeId>,
    /// Interned name → NodeFieldId (for binary: StringId → NodeFieldId)
    pub node_field_ids: IndexMap<Symbol, NodeFieldId>,
}

use super::symbol_table::SymbolTable;
use super::utils::find_similar;
use super::visitor::{Visitor, walk};
use crate::diagnostics::{DiagnosticKind, Diagnostics};
use crate::parser::ast::{self, Expr, NamedNode};
use crate::parser::{SyntaxKind, SyntaxToken, token_src};
use crate::query::{AstMap, SourceId, SourceMap};

/// Link query against a language grammar.
///
/// This function is decoupled from `Query` to allow easier testing and
/// modularity. It orchestrates the resolution and validation phases.
pub fn link<'q>(
    interner: &mut Interner,
    grammar: &Grammar,
    source_map: &'q SourceMap,
    ast_map: &AstMap,
    symbol_table: &SymbolTable,
    output: &mut LinkOutput,
    diagnostics: &mut Diagnostics,
) {
    // Local deduplication maps (not exposed in output)
    let mut node_type_ids: HashMap<NodeType<&'q str>, Option<NodeTypeId>> = HashMap::new();
    let mut node_field_ids: HashMap<&'q str, Option<NodeFieldId>> = HashMap::new();

    for (&source_id, root) in ast_map {
        let mut linker = Linker {
            interner,
            grammar,
            source_map,
            symbol_table,
            source_id,
            node_type_ids: &mut node_type_ids,
            node_field_ids: &mut node_field_ids,
            output,
            diagnostics,
        };
        linker.link(root);
    }
}

struct Linker<'a, 'q> {
    // Refs
    interner: &'a mut Interner,
    grammar: &'a Grammar,
    source_map: &'q SourceMap,
    symbol_table: &'a SymbolTable,
    source_id: SourceId,
    node_type_ids: &'a mut HashMap<NodeType<&'q str>, Option<NodeTypeId>>,
    node_field_ids: &'a mut HashMap<&'q str, Option<NodeFieldId>>,
    output: &'a mut LinkOutput,
    diagnostics: &'a mut Diagnostics,
}

impl<'a, 'q> Linker<'a, 'q> {
    fn source(&self) -> &'q str {
        self.source_map.content(self.source_id)
    }

    fn link(&mut self, root: &ast::Root) {
        self.resolve_symbols(root);
        self.validate_structure(root);
    }

    fn resolve_symbols(&mut self, root: &ast::Root) {
        let mut resolver = SymbolResolver { linker: self };
        resolver.visit(root);
    }

    fn resolve_named_node(&mut self, node: &NamedNode) {
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
        let key = NodeType::Named(token_src(&type_token, self.source()));
        if self.node_type_ids.contains_key(&key) {
            return;
        }
        let resolved = self.grammar.resolve_named_node(type_name);
        self.node_type_ids.insert(key, resolved);
        if let Some(id) = resolved {
            let sym = self.interner.intern(type_name);
            self.output
                .node_type_ids
                .entry(NodeType::Named(sym))
                .or_insert(id);
        }
        if resolved.is_none() {
            let all_types = self.grammar.all_named_node_kinds();
            let max_dist = (type_name.len() / 3).clamp(2, 4);
            let suggestion = find_similar(type_name, &all_types, max_dist);

            let mut builder = self
                .diagnostics
                .report(
                    self.source_id,
                    DiagnosticKind::UnknownNodeType,
                    type_token.text_range(),
                )
                .message(type_name);

            if let Some(similar) = suggestion {
                builder = builder.hint(format!("did you mean `{}`?", similar));
            }
            builder.emit();
        }
    }

    fn resolve_field_by_token(&mut self, name_token: Option<SyntaxToken>) {
        let Some(name_token) = name_token else {
            return;
        };
        let field_name = name_token.text();
        if self.node_field_ids.contains_key(field_name) {
            return;
        }
        let resolved = self.grammar.resolve_field(field_name);
        self.node_field_ids
            .insert(token_src(&name_token, self.source()), resolved);
        if let Some(id) = resolved {
            let sym = self.interner.intern(field_name);
            self.output.node_field_ids.entry(sym).or_insert(id);
            return;
        }
        let all_fields = self.grammar.all_field_names();
        let max_dist = (field_name.len() / 3).clamp(2, 4);
        let suggestion = find_similar(field_name, &all_fields, max_dist);

        let mut builder = self
            .diagnostics
            .report(
                self.source_id,
                DiagnosticKind::UnknownField,
                name_token.text_range(),
            )
            .message(field_name);

        if let Some(similar) = suggestion {
            builder = builder.hint(format!("did you mean `{}`?", similar));
        }
        builder.emit();
    }

    fn validate_structure(&mut self, root: &ast::Root) {
        let defs: Vec<_> = root.defs().collect();
        for def in defs {
            let Some(body) = def.body() else { continue };
            let mut walk = RefWalk::default();
            self.validate_expr_structure(&body, None, &mut walk);
        }
    }

    fn validate_expr_structure(
        &mut self,
        expr: &Expr,
        ctx: Option<ValidationContext>,
        walk: &mut RefWalk,
    ) {
        match expr {
            Expr::NamedNode(node) => {
                let child_ctx = self.make_node_context(node);

                // Predicates are only valid on leaf nodes (grammar check)
                if let Some(pred) = node.predicate()
                    && let Some(ctx) = &child_ctx
                    && (!self.grammar.valid_child_types(ctx.parent_id).is_empty()
                        || !self.grammar.fields_for_node_type(ctx.parent_id).is_empty())
                {
                    self.diagnostics
                        .report(
                            self.source_id,
                            DiagnosticKind::PredicateOnNonLeaf,
                            pred.as_cst().text_range(),
                        )
                        .emit();
                }

                for child in node.children() {
                    if let Expr::FieldExpr(f) = &child {
                        self.validate_field_expr(f, child_ctx.as_ref(), walk);
                    } else {
                        self.validate_expr_structure(&child, child_ctx, walk);
                    }
                }

                if let Some(ctx) = child_ctx {
                    for child in node.as_cst().children() {
                        if let Some(neg) = ast::NegatedField::cast(child) {
                            self.validate_negated_field(&neg, &ctx);
                        }
                    }
                }
            }
            Expr::AnonymousNode(_) => {}
            Expr::FieldExpr(f) => {
                // Should be handled by parent NamedNode, but handle gracefully
                self.validate_field_expr(f, ctx.as_ref(), walk);
            }
            Expr::AltExpr(alt) => {
                for branch in alt.branches() {
                    let Some(body) = branch.body() else { continue };
                    self.validate_expr_structure(&body, ctx, walk);
                }
            }
            Expr::SeqExpr(seq) => {
                for child in seq.children() {
                    self.validate_expr_structure(&child, ctx, walk);
                }
            }
            Expr::CapturedExpr(cap) => {
                let Some(inner) = cap.inner() else { return };
                self.validate_expr_structure(&inner, ctx, walk);
            }
            Expr::QuantifiedExpr(q) => {
                let Some(inner) = q.inner() else { return };
                self.validate_expr_structure(&inner, ctx, walk);
            }
            Expr::Ref(r) => {
                let Some(name_token) = r.name() else { return };
                let name = name_token.text();
                // Validation is a pure function of `(name, ctx)`, so caching it
                // collapses diamond-shaped reference graphs that would otherwise
                // be re-walked 2^depth times. Cut cycles are never cached: they
                // return below without reaching the `validated.insert`.
                let key = (name.to_string(), ctx);
                if walk.validated.contains(&key) {
                    return;
                }
                if !walk.in_progress.insert(name.to_string()) {
                    return;
                }
                let Some((ref_source, body)) = self
                    .symbol_table
                    .get_full(name)
                    .map(|(s, b)| (s, b.clone()))
                else {
                    walk.in_progress.remove(name);
                    return;
                };
                // The referenced definition may live in another workspace file.
                // Validate its body under ITS source so token slicing and
                // diagnostics resolve against the right content.
                let saved_source = self.source_id;
                self.source_id = ref_source;
                self.validate_expr_structure(&body, ctx, walk);
                self.source_id = saved_source;
                walk.in_progress.remove(name);
                walk.validated.insert(key);
            }
        }
    }

    /// Create validation context for a named node's children.
    fn make_node_context(&self, node: &NamedNode) -> Option<ValidationContext> {
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
        let key = NodeType::Named(token_src(&type_token, self.source()));
        let parent_id = self.node_type_ids.get(&key).copied().flatten()?;
        // Verify the node type exists in the grammar
        self.grammar.node_type_name(parent_id)?;
        Some(ValidationContext {
            parent_id,
            parent_range: type_token.text_range(),
            parent_source: self.source_id,
        })
    }

    fn validate_field_expr(
        &mut self,
        field: &ast::FieldExpr,
        ctx: Option<&ValidationContext>,
        walk: &mut RefWalk,
    ) {
        let Some(name_token) = field.name() else {
            return;
        };
        let Some(field_id) = self
            .node_field_ids
            .get(name_token.text())
            .copied()
            .flatten()
        else {
            return;
        };
        let Some(ctx) = ctx else { return };

        if !self.grammar.has_field(ctx.parent_id, field_id) {
            self.emit_field_not_on_node(
                name_token.text_range(),
                name_token.text(),
                ctx.parent_id,
                ctx.parent_range,
                ctx.parent_source,
            );
            return;
        }

        let Some(value) = field.value() else { return };
        self.validate_expr_structure(&value, Some(*ctx), walk);
    }

    fn validate_negated_field(&mut self, neg: &ast::NegatedField, ctx: &ValidationContext) {
        let Some(name_token) = neg.name() else {
            return;
        };
        let field_name = name_token.text();

        let Some(field_id) = self.node_field_ids.get(field_name).copied().flatten() else {
            return;
        };

        if self.grammar.has_field(ctx.parent_id, field_id) {
            return;
        }
        self.emit_field_not_on_node(
            name_token.text_range(),
            field_name,
            ctx.parent_id,
            ctx.parent_range,
            ctx.parent_source,
        );
    }

    fn emit_field_not_on_node(
        &mut self,
        range: TextRange,
        field_name: &str,
        parent_id: NodeTypeId,
        parent_range: TextRange,
        parent_source: SourceId,
    ) {
        let valid_fields = self.grammar.fields_for_node_type(parent_id);
        let parent_name = self
            .grammar
            .node_type_name(parent_id)
            .expect("validated parent_id must have a name");

        let mut builder = self
            .diagnostics
            .report(self.source_id, DiagnosticKind::FieldNotOnNodeType, range)
            .message(field_name)
            .related_to(parent_source, parent_range, format!("on `{}`", parent_name));

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
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct ValidationContext {
    /// The parent node type being validated against.
    parent_id: NodeTypeId,
    /// The parent node type token range for related_to.
    parent_range: TextRange,
    /// Source the parent node lives in. May differ from the source currently
    /// being walked once validation crosses a reference into another workspace
    /// file, so `parent_range` must be reported against this, not `self.source_id`.
    parent_source: SourceId,
}

/// State for walking the reference graph during structural validation.
#[derive(Default)]
struct RefWalk {
    /// Definitions currently on the recursion stack — guards against cycles.
    in_progress: HashSet<String>,
    /// Definitions already validated under a given context. A definition's
    /// validation depends only on `(name, ctx)`, so caching it keeps shared
    /// references (e.g. diamond graphs) from being re-walked exponentially.
    validated: HashSet<(String, Option<ValidationContext>)>,
}

/// Combined symbol resolver for node types and fields.
struct SymbolResolver<'l, 'a, 'q> {
    linker: &'l mut Linker<'a, 'q>,
}

impl Visitor for SymbolResolver<'_, '_, '_> {
    fn visit(&mut self, root: &ast::Root) {
        walk(self, root);
    }

    fn visit_named_node(&mut self, node: &ast::NamedNode) {
        self.linker.resolve_named_node(node);

        for neg in node.as_cst().children().filter_map(ast::NegatedField::cast) {
            self.linker.resolve_field_by_token(neg.name());
        }

        super::visitor::walk_named_node(self, node);
    }

    fn visit_anonymous_node(&mut self, node: &ast::AnonymousNode) {
        if node.is_any() {
            return;
        }
        let Some(value_token) = node.value() else {
            return;
        };
        let value = value_token.text();
        let key = NodeType::Anonymous(token_src(&value_token, self.linker.source()));
        if self.linker.node_type_ids.contains_key(&key) {
            return;
        }

        let resolved = self.linker.grammar.resolve_anonymous_node(value);
        self.linker.node_type_ids.insert(key, resolved);

        if let Some(id) = resolved {
            let sym = self.linker.interner.intern(value);
            self.linker
                .output
                .node_type_ids
                .entry(NodeType::Anonymous(sym))
                .or_insert(id);
            return;
        }

        self.linker
            .diagnostics
            .report(
                self.linker.source_id,
                DiagnosticKind::UnknownNodeType,
                value_token.text_range(),
            )
            .message(value)
            .emit();
    }

    fn visit_field_expr(&mut self, field: &ast::FieldExpr) {
        self.linker.resolve_field_by_token(field.name());
        super::visitor::walk_field_expr(self, field);
    }
}
