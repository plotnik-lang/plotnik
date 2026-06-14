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
            self.validate_expr_structure(&body, None, false, &mut walk);
        }
    }

    /// Walk the query, validating each node's own grammar constraints. `deferred` is set once the
    /// walk descends into an alternation branch or a quantified body: inside those, nothing is
    /// guaranteed to participate in a match (a sibling branch or zero repetitions can satisfy the
    /// query), so the grammar checks must NOT fire there — doing so would reject queries that can
    /// match. Skipping a check can only miss a rejection, never reject a valid query.
    fn validate_expr_structure(
        &mut self,
        expr: &Expr,
        ctx: Option<ValidationContext>,
        deferred: bool,
        walk: &mut RefWalk,
    ) {
        match expr {
            Expr::NamedNode(node) => {
                let child_ctx = self.make_node_context(node);

                // A `#subtype` refinement must denote a satisfiable kind of its base type.
                if !deferred {
                    self.validate_subtype(node);
                }

                // Predicates are only valid on leaf nodes. Skipped under a disjunction/option,
                // where this position need not match for the query to.
                if !deferred
                    && let Some(pred) = node.predicate()
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

                // The set of child kinds the grammar can place under this parent, computed once
                // once for the inadmissible-child and child-under-leaf-token diagnostics.
                let parent_admissibility = child_ctx.as_ref().map(|ctx| {
                    (
                        self.admissible_set(ctx.parent_id),
                        self.grammar.is_token(ctx.parent_id),
                    )
                });

                for child in node.children() {
                    if let Expr::FieldExpr(f) = &child {
                        self.validate_field_expr(f, child_ctx.as_ref(), deferred, walk);
                    } else {
                        if !deferred
                            && let (Some(ctx), Some((adm, parent_is_token))) =
                                (child_ctx.as_ref(), parent_admissibility.as_ref())
                        {
                            self.check_bare_child(&child, ctx, *parent_is_token, adm);
                        }
                        self.validate_expr_structure(&child, child_ctx, deferred, walk);
                    }
                }

                if let Some(ctx) = child_ctx {
                    for child in node.as_cst().children() {
                        if let Some(neg) = ast::NegatedField::cast(child) {
                            self.validate_negated_field(&neg, &ctx, deferred);
                        }
                    }
                }
            }
            Expr::AnonymousNode(_) => {}
            Expr::FieldExpr(f) => {
                // Should be handled by parent NamedNode, but handle gracefully
                self.validate_field_expr(f, ctx.as_ref(), deferred, walk);
            }
            Expr::AltExpr(alt) => {
                // A branch is disjunctive — none is guaranteed to match, so defer its contents.
                for branch in alt.branches() {
                    let Some(body) = branch.body() else { continue };
                    self.validate_expr_structure(&body, ctx, true, walk);
                }
            }
            Expr::SeqExpr(seq) => {
                for child in seq.children() {
                    self.validate_expr_structure(&child, ctx, deferred, walk);
                }
            }
            Expr::CapturedExpr(cap) => {
                let Some(inner) = cap.inner() else { return };
                self.validate_expr_structure(&inner, ctx, deferred, walk);
            }
            Expr::QuantifiedExpr(q) => {
                let Some(inner) = q.inner() else { return };
                // The body is optional/repeated — zero occurrences can satisfy it, so defer.
                self.validate_expr_structure(&inner, ctx, true, walk);
            }
            Expr::Ref(r) => {
                let Some(name_token) = r.name() else { return };
                let name = name_token.text();
                // Validation is a pure function of `(name, ctx, deferred)`, so caching it
                // collapses diamond-shaped reference graphs that would otherwise be re-walked
                // 2^depth times. `deferred` is part of the key: a definition reached both inside
                // and outside an alternation/quantifier must still be checked in its non-deferred
                // context even after the deferred reach cached it. Cut cycles are never cached:
                // they return below without reaching the `validated.insert`.
                let key = (name.to_string(), ctx, deferred);
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
                self.validate_expr_structure(&body, ctx, deferred, walk);
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
        deferred: bool,
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
            // A field absent from this kind can never match here, but a sibling branch or zero
            // repetitions can — so skip when deferred.
            if !deferred {
                self.emit_field_not_on_node(
                    name_token.text_range(),
                    name_token.text(),
                    ctx.parent_id,
                    ctx.parent_range,
                    ctx.parent_source,
                );
            }
            return;
        }

        let Some(value) = field.value() else { return };
        // The field value's kind must be admissible for this field. Skipped under a
        // disjunction/option, where the field constraint need not hold for the query to match.
        if !deferred {
            self.check_field_value(
                &value,
                ctx,
                field_id,
                name_token.text(),
                name_token.text_range(),
            );
        }
        self.validate_expr_structure(&value, Some(*ctx), deferred, walk);
    }

    fn validate_negated_field(
        &mut self,
        neg: &ast::NegatedField,
        ctx: &ValidationContext,
        deferred: bool,
    ) {
        let Some(name_token) = neg.name() else {
            return;
        };
        let field_name = name_token.text();

        let Some(field_id) = self.node_field_ids.get(field_name).copied().flatten() else {
            return;
        };

        if !self.grammar.has_field(ctx.parent_id, field_id) {
            if !deferred {
                self.emit_field_not_on_node(
                    name_token.text_range(),
                    field_name,
                    ctx.parent_id,
                    ctx.parent_range,
                    ctx.parent_source,
                );
            }
            return;
        }

        // A required field is present in every production, so asserting its absence can never
        // match. Skipped under a disjunction/option, where the negation need not hold.
        if !deferred
            && self
                .grammar
                .field_cardinality(ctx.parent_id, field_id)
                .is_some_and(|cardinality| cardinality.required)
        {
            let parent_name = self
                .grammar
                .node_type_name(ctx.parent_id)
                .expect("validated parent_id must have a name");
            self.diagnostics
                .report(
                    self.source_id,
                    DiagnosticKind::NegatedRequiredField,
                    name_token.text_range(),
                )
                .message(field_name)
                .related_to(
                    self.source_id,
                    ctx.parent_range,
                    format!("on `{}`", parent_name),
                )
                .hint(format!(
                    "`-{0}` requires `{0}` to be absent, but every `{1}` has one — drop `-{0}`",
                    field_name, parent_name
                ))
                .emit();
        }
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

    /// Resolve a child/value `NamedNode` to its grammar id, mirroring `make_node_context` but
    /// returning just the id. `None` for `(_)`, `ERROR`, `MISSING`, or an unresolved kind
    /// (the latter already reported by the resolution pass) — all of which carry no
    /// admissibility signal and are conservatively accepted.
    fn resolve_named_node_id(&self, node: &NamedNode) -> Option<NodeTypeId> {
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
        self.node_type_ids.get(&key).copied().flatten()
    }

    /// All child kinds (named children and field values) the grammar can place under `parent`,
    /// expanded through supertype subtyping in both directions. A bare child is admissible iff
    /// it lands in this set (or is an extra / a supertype overlapping it).
    fn admissible_set(&self, parent: NodeTypeId) -> HashSet<NodeTypeId> {
        let mut seeds = self.grammar.valid_child_types(parent).to_vec();
        for field_name in self.grammar.fields_for_node_type(parent) {
            if let Some(field_id) = self.grammar.resolve_field(field_name) {
                seeds.extend_from_slice(self.grammar.valid_field_types(parent, field_id));
            }
        }

        let mut admissible = HashSet::new();
        for seed in seeds {
            admissible.insert(seed);
            admissible.extend(self.grammar.collect_subtypes(seed));
        }
        admissible
    }

    /// Whether a concrete child kind can occupy a bare child position whose parent admits
    /// `adm`. The parent is already known to be a non-leaf here.
    fn admissible_child(&self, child: NodeTypeId, adm: &HashSet<NodeTypeId>) -> bool {
        adm.contains(&child)
            || self.grammar.is_extra(child)
            || (self.grammar.is_supertype(child)
                && self
                    .grammar
                    .collect_subtypes(child)
                    .iter()
                    .any(|kind| adm.contains(kind)))
    }

    /// Validate one bare (non-field) child position against its parent. Descends the always-present
    /// wrappers (capture, sequence) and reports at the deepest pinned leaf; alternations,
    /// quantifiers, and references are skipped — their satisfiability is not checked here, and
    /// skipping can only miss a rejection, never reject a valid query.
    fn check_bare_child(
        &mut self,
        expr: &Expr,
        ctx: &ValidationContext,
        parent_is_token: bool,
        adm: &HashSet<NodeTypeId>,
    ) {
        match expr {
            Expr::CapturedExpr(cap) => {
                if let Some(inner) = cap.inner() {
                    self.check_bare_child(&inner, ctx, parent_is_token, adm);
                }
            }
            Expr::SeqExpr(seq) => {
                for child in seq.children() {
                    self.check_bare_child(&child, ctx, parent_is_token, adm);
                }
            }
            Expr::NamedNode(node) => {
                self.check_bare_named_child(node, ctx, parent_is_token, adm);
            }
            // Anonymous children are untracked (grammar children arrays never list anonymous
            // tokens). Alternations, quantifiers, and references are not checked here.
            Expr::AnonymousNode(_)
            | Expr::AltExpr(_)
            | Expr::QuantifiedExpr(_)
            | Expr::Ref(_)
            | Expr::FieldExpr(_) => {}
        }
    }

    fn check_bare_named_child(
        &mut self,
        node: &NamedNode,
        ctx: &ValidationContext,
        parent_is_token: bool,
        adm: &HashSet<NodeTypeId>,
    ) {
        // `(_)` matches any named node, so it is impossible only beneath a leaf token.
        if node.is_any() {
            if parent_is_token {
                self.emit_child_under_leaf_token(
                    node.text_range(),
                    ctx.parent_id,
                    ctx.parent_range,
                );
            }
            return;
        }

        let Some(child_id) = self.resolve_named_node_id(node) else {
            return;
        };
        let Some(type_token) = node.node_type() else {
            return;
        };

        if parent_is_token {
            // A leaf token has no child nodes, so any named child is impossible.
            self.emit_child_under_leaf_token(
                type_token.text_range(),
                ctx.parent_id,
                ctx.parent_range,
            );
            return;
        }

        // The kind is not among the parent's admissible children.
        if !self.admissible_child(child_id, adm) {
            self.emit_invalid_child(
                type_token.text_range(),
                child_id,
                ctx.parent_id,
                ctx.parent_range,
            );
        }
    }

    /// Validate one field value against the field's admissible types. Mirrors `check_bare_child`
    /// but uses the field's type set and has no extras/leaf-token rescue (fields hold specific
    /// kinds, never comments).
    fn check_field_value(
        &mut self,
        expr: &Expr,
        ctx: &ValidationContext,
        field_id: NodeFieldId,
        field_name: &str,
        field_range: TextRange,
    ) {
        match expr {
            Expr::CapturedExpr(cap) => {
                if let Some(inner) = cap.inner() {
                    self.check_field_value(&inner, ctx, field_id, field_name, field_range);
                }
            }
            Expr::NamedNode(node) => {
                self.check_field_named_value(node, ctx, field_id, field_name, field_range);
            }
            Expr::AnonymousNode(anon) => {
                self.check_field_anon_value(anon, ctx, field_id, field_name, field_range);
            }
            // Alternations, quantifiers, and references are not checked here; a field value
            // can't be a sequence (rejected earlier as `FieldSequenceValue`).
            Expr::AltExpr(_)
            | Expr::QuantifiedExpr(_)
            | Expr::Ref(_)
            | Expr::SeqExpr(_)
            | Expr::FieldExpr(_) => {}
        }
    }

    fn check_field_named_value(
        &mut self,
        node: &NamedNode,
        ctx: &ValidationContext,
        field_id: NodeFieldId,
        field_name: &str,
        field_range: TextRange,
    ) {
        if node.is_any() {
            // `(_)` matches any named node — impossible only when the field admits literal
            // tokens exclusively.
            if self.field_is_anonymous_only(ctx.parent_id, field_id) {
                let message = format!("a named node can't be the value of `{}`", field_name);
                self.emit_invalid_field_value(
                    node.text_range(),
                    message,
                    ctx.parent_id,
                    field_id,
                    field_name,
                    field_range,
                );
            }
            return;
        }

        let Some(value_id) = self.resolve_named_node_id(node) else {
            return;
        };
        let Some(type_token) = node.node_type() else {
            return;
        };

        if self.field_admissible(value_id, ctx.parent_id, field_id) {
            return;
        }

        let value_name = self
            .grammar
            .node_type_name(value_id)
            .expect("resolved value must have a name");
        let message = format!("`{}` can't be the value of `{}`", value_name, field_name);
        self.emit_invalid_field_value(
            type_token.text_range(),
            message,
            ctx.parent_id,
            field_id,
            field_name,
            field_range,
        );
    }

    fn check_field_anon_value(
        &mut self,
        anon: &ast::AnonymousNode,
        ctx: &ValidationContext,
        field_id: NodeFieldId,
        field_name: &str,
        field_range: TextRange,
    ) {
        // The bare `_` matches any node, anonymous tokens included, so it always fits.
        if anon.is_any() {
            return;
        }
        let Some(value_token) = anon.value() else {
            return;
        };
        let key = NodeType::Anonymous(token_src(&value_token, self.source()));
        let Some(value_id) = self.node_type_ids.get(&key).copied().flatten() else {
            return;
        };

        if self
            .field_admissible_set(ctx.parent_id, field_id)
            .contains(&value_id)
        {
            return;
        }

        let value_name = value_token.text().to_string();
        let message = format!("`{}` can't be the value of `{}`", value_name, field_name);
        self.emit_invalid_field_value(
            value_token.text_range(),
            message,
            ctx.parent_id,
            field_id,
            field_name,
            field_range,
        );
    }

    /// Field value types expanded through supertype subtyping.
    fn field_admissible_set(
        &self,
        parent: NodeTypeId,
        field_id: NodeFieldId,
    ) -> HashSet<NodeTypeId> {
        let mut admissible = HashSet::new();
        for &seed in self.grammar.valid_field_types(parent, field_id) {
            admissible.insert(seed);
            admissible.extend(self.grammar.collect_subtypes(seed));
        }
        admissible
    }

    fn field_admissible(
        &self,
        value: NodeTypeId,
        parent: NodeTypeId,
        field_id: NodeFieldId,
    ) -> bool {
        let admissible = self.field_admissible_set(parent, field_id);
        admissible.contains(&value)
            || (self.grammar.is_supertype(value)
                && self
                    .grammar
                    .collect_subtypes(value)
                    .iter()
                    .any(|kind| admissible.contains(kind)))
    }

    /// True only when every type the field accepts is a literal token — then a `(_)` (named)
    /// value is impossible. Errs toward `false` (accept) when any type isn't confidently
    /// anonymous, keeping rejection sound.
    fn field_is_anonymous_only(&self, parent: NodeTypeId, field_id: NodeFieldId) -> bool {
        let types = self.grammar.valid_field_types(parent, field_id);
        !types.is_empty() && types.iter().all(|&id| self.grammar.is_anonymous_node(id))
    }

    /// Whether two kinds can denote the same concrete node — i.e. their subtype closures (each
    /// including the kind itself) intersect. Even sibling supertypes can share a concrete member,
    /// so a non-empty overlap is what makes a `(super#sub)` refinement satisfiable.
    fn subtypes_overlap(&self, a: NodeTypeId, b: NodeTypeId) -> bool {
        if a == b {
            return true;
        }
        let mut a_members = self.grammar.collect_subtypes(a);
        a_members.insert(a);
        if a_members.contains(&b) {
            return true;
        }
        let mut b_members = self.grammar.collect_subtypes(b);
        b_members.insert(b);
        a_members.iter().any(|member| b_members.contains(member))
    }

    /// A `(supertype#subtype)` refinement must be satisfiable — its base and refinement subtype
    /// closures must overlap.
    fn validate_subtype(&mut self, node: &NamedNode) {
        let Some(sub_token) = node.subtype() else {
            return;
        };
        let Some(super_token) = node.node_type() else {
            return;
        };
        if matches!(
            super_token.kind(),
            SyntaxKind::Underscore | SyntaxKind::KwError | SyntaxKind::KwMissing
        ) {
            return;
        }
        let Some(super_id) = self.resolve_named_node_id(node) else {
            return;
        };

        let sub_name = sub_token.text();
        let Some(sub_id) = self.grammar.resolve_named_node(sub_name) else {
            let all_types = self.grammar.all_named_node_kinds();
            let max_dist = (sub_name.len() / 3).clamp(2, 4);
            let suggestion = find_similar(sub_name, &all_types, max_dist).map(str::to_string);
            let mut builder = self
                .diagnostics
                .report(
                    self.source_id,
                    DiagnosticKind::UnknownNodeType,
                    sub_token.text_range(),
                )
                .message(sub_name);
            if let Some(similar) = suggestion {
                builder = builder.hint(format!("did you mean `{}`?", similar));
            }
            builder.emit();
            return;
        };

        // Refining a concrete kind is meaningless, but accept it. For a supertype base, the
        // refinement is impossible only when no concrete node can be both — i.e. when the two
        // subtype closures are disjoint. Testing `sub ∈ subtypes(super)` alone over-rejects when
        // `sub` is itself a supertype that overlaps `super` (e.g. C# `preproc_if` is both a
        // `statement` and a `declaration`, so `(statement#declaration)` can match).
        if !self.grammar.is_supertype(super_id) || self.subtypes_overlap(super_id, sub_id) {
            return;
        }

        let super_name = self
            .grammar
            .node_type_name(super_id)
            .expect("resolved supertype must have a name")
            .to_string();
        let kinds = self
            .grammar
            .subtypes(super_id)
            .iter()
            .filter_map(|&id| self.grammar.node_type_name(id))
            .collect::<Vec<_>>();
        let kinds_hint = (!kinds.is_empty()).then(|| {
            format!(
                "kinds of `{}` include: {}",
                super_name,
                format_list(&kinds, 8)
            )
        });

        let mut builder = self
            .diagnostics
            .report(
                self.source_id,
                DiagnosticKind::InvalidSubtype,
                sub_token.text_range(),
            )
            .message(format!("`{}` is not a kind of `{}`", sub_name, super_name))
            .related_to(
                self.source_id,
                super_token.text_range(),
                format!("base type `{}`", super_name),
            );
        if let Some(hint) = kinds_hint {
            builder = builder.hint(hint);
        }
        builder.emit();
    }

    /// Emit an inadmissible-bare-child diagnostic with a children-vs-fields hint.
    fn emit_invalid_child(
        &mut self,
        range: TextRange,
        child_id: NodeTypeId,
        parent_id: NodeTypeId,
        parent_range: TextRange,
    ) {
        let child_name = self
            .grammar
            .node_type_name(child_id)
            .expect("resolved child must have a name")
            .to_string();
        let parent_name = self
            .grammar
            .node_type_name(parent_id)
            .expect("validated parent_id must have a name")
            .to_string();
        let hint = self.child_hint(parent_id, &parent_name);

        self.diagnostics
            .report(self.source_id, DiagnosticKind::InvalidChildType, range)
            .message(child_name)
            .related_to(
                self.source_id,
                parent_range,
                format!("on `{}`", parent_name),
            )
            .hint(hint)
            .emit();
    }

    /// Emit a child-under-leaf-token diagnostic.
    fn emit_child_under_leaf_token(
        &mut self,
        range: TextRange,
        parent_id: NodeTypeId,
        parent_range: TextRange,
    ) {
        let parent_name = self
            .grammar
            .node_type_name(parent_id)
            .expect("validated parent_id must have a name")
            .to_string();

        self.diagnostics
            .report(self.source_id, DiagnosticKind::ChildUnderLeafToken, range)
            .message(&parent_name)
            .related_to(self.source_id, parent_range, format!("`{}`", parent_name))
            .hint(format!(
                "a leaf token's content is its text — match it directly `({0})` or by value `({0} == \"foo\")`",
                parent_name
            ))
            .emit();
    }

    /// Hint for the inadmissible-child diagnostic: list valid unlabeled children, or — when a
    /// node's only children are field values — surface those as fields so users don't write ghost
    /// bare-child queries.
    fn child_hint(&self, parent_id: NodeTypeId, parent_name: &str) -> String {
        let child_types = self.grammar.valid_child_types(parent_id);
        if !child_types.is_empty() {
            let names = child_types
                .iter()
                .filter_map(|&id| self.grammar.node_type_name(id))
                .collect::<Vec<_>>();
            return format!(
                "valid children of `{}`: {}",
                parent_name,
                format_list(&names, 8)
            );
        }

        let fields = self.grammar.fields_for_node_type(parent_id);
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

    /// Render a field as `name: (type)` using its first valid type, for child/field hints.
    fn render_field(&self, parent_id: NodeTypeId, field_name: &str) -> String {
        let type_name = self
            .grammar
            .resolve_field(field_name)
            .map(|field_id| self.grammar.valid_field_types(parent_id, field_id))
            .unwrap_or(&[])
            .iter()
            .find_map(|&id| self.grammar.node_type_name(id))
            .unwrap_or("_");
        format!("`{}: ({})`", field_name, type_name)
    }

    /// Emit an invalid-field-value diagnostic with an accepts-list hint.
    fn emit_invalid_field_value(
        &mut self,
        range: TextRange,
        message: String,
        parent_id: NodeTypeId,
        field_id: NodeFieldId,
        field_name: &str,
        field_range: TextRange,
    ) {
        let hint = self.field_value_hint(parent_id, field_id, field_name);
        self.diagnostics
            .report(self.source_id, DiagnosticKind::InvalidFieldChildType, range)
            .message(message)
            .related_to(
                self.source_id,
                field_range,
                format!("field `{}`", field_name),
            )
            .hint(hint)
            .emit();
    }

    /// Hint for the invalid-field-value diagnostic: the named kinds a field accepts, or — for
    /// literal-only fields — a concrete `field: "token"` example.
    fn field_value_hint(
        &self,
        parent_id: NodeTypeId,
        field_id: NodeFieldId,
        field_name: &str,
    ) -> String {
        let types = self.grammar.valid_field_types(parent_id, field_id);
        let named = types
            .iter()
            .filter(|&&id| !self.grammar.is_anonymous_node(id))
            .filter_map(|&id| self.grammar.node_type_name(id))
            .collect::<Vec<_>>();

        if named.is_empty() {
            let example = types
                .iter()
                .find_map(|&id| self.grammar.node_type_name(id))
                .unwrap_or("…");
            return format!(
                "`{0}` accepts only literal tokens — write `{0}: \"{1}\"`",
                field_name, example
            );
        }
        format!("`{}` accepts: {}", field_name, format_list(&named, 8))
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
    /// validation depends only on `(name, ctx, deferred)`, so caching it keeps shared
    /// references (e.g. diamond graphs) from being re-walked exponentially.
    validated: HashSet<(String, Option<ValidationContext>, bool)>,
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
