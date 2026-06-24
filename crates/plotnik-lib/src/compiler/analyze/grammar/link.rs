//! Link pass: resolve node kinds and fields against tree-sitter grammar.
//!
//! Two-phase approach:
//! 1. Resolve all symbols (node kinds and fields) against grammar
//! 2. Validate structural constraints (field on node kind, child kind for field)

use std::collections::{HashMap, HashSet};

use crate::core::grammar::Grammar;
use crate::core::{Interner, NodeFieldId, NodeKind, NodeKindId};
use indexmap::IndexMap;
use rowan::TextRange;

use super::grammar_binding::GrammarBindingBuilder;
use super::utils::find_similar;
use crate::compiler::analyze::Located;
use crate::compiler::parse::ast::Root;
use crate::compiler::analyze::names::SymbolTable;
use crate::compiler::parse::ast::{self, NodePattern, Pattern};
use crate::compiler::diagnostics::source::{SourceId, SourceMap};
use crate::compiler::analyze::visitor::{Visitor, walk};
use crate::compiler::parse::cst::{SyntaxKind, SyntaxToken};
use crate::compiler::parse::ast::token_src;
use crate::compiler::diagnostics::diagnostics::{DiagnosticKind, Diagnostics, Span};

/// The threaded dependencies of the link pass. Decoupled from `Query` to allow
/// testing without a full query context.
pub struct GrammarLinkCtx<'a, 'q> {
    pub interner: &'a mut Interner,
    pub grammar: &'a Grammar,
    pub source_map: &'q SourceMap,
    pub ast_map: &'q IndexMap<SourceId, Root>,
    pub symbol_table: &'a SymbolTable,
}

impl<'q> GrammarLinkCtx<'_, 'q> {
    pub(crate) fn link(self, output: &mut GrammarBindingBuilder, diagnostics: &mut Diagnostics) {
        // Local deduplication maps (not exposed in output)
        let mut node_kind_ids: HashMap<NodeKind<&'q str>, Option<NodeKindId>> = HashMap::new();
        let mut node_field_ids: HashMap<&'q str, Option<NodeFieldId>> = HashMap::new();

        for (&source_id, root) in self.ast_map {
            let mut linker = GrammarLinker {
                interner: &mut *self.interner,
                grammar: self.grammar,
                source_map: self.source_map,
                symbol_table: self.symbol_table,
                node_kind_ids: &mut node_kind_ids,
                node_field_ids: &mut node_field_ids,
                output,
                diag: diagnostics,
            };
            linker.link(source_id, root);
        }
    }
}

struct GrammarLinker<'a, 'q> {
    // Refs
    interner: &'a mut Interner,
    grammar: &'a Grammar,
    source_map: &'q SourceMap,
    symbol_table: &'a SymbolTable,
    node_kind_ids: &'a mut HashMap<NodeKind<&'q str>, Option<NodeKindId>>,
    node_field_ids: &'a mut HashMap<&'q str, Option<NodeFieldId>>,
    output: &'a mut GrammarBindingBuilder,
    diag: &'a mut Diagnostics,
}

impl<'a, 'q> GrammarLinker<'a, 'q> {
    fn content(&self, source: SourceId) -> &'q str {
        self.source_map.content(source)
    }

    fn link(&mut self, source: SourceId, root: &ast::Root) {
        self.resolve_symbols(source, root);
        self.check_grammar(source, root);
    }

    fn resolve_symbols(&mut self, source: SourceId, root: &ast::Root) {
        let mut resolver = SymbolResolver { linker: self };
        resolver.visit(&Located::new(source, root.clone()));
    }

    fn resolve_named_node(&mut self, located: &Located<NodePattern>) {
        let node = located.node();
        if node.is_any() {
            return;
        }
        let Some(type_token) = node.kind_token() else {
            return;
        };
        if matches!(
            type_token.kind(),
            SyntaxKind::KwError | SyntaxKind::KwMissing
        ) {
            return;
        }
        let type_name = type_token.text();
        let key = NodeKind::Named(token_src(&type_token, self.content(located.source())));
        if self.node_kind_ids.contains_key(&key) {
            return;
        }
        let resolved = self.grammar.resolve_named_node(type_name);
        self.node_kind_ids.insert(key, resolved);
        if let Some(id) = resolved {
            let sym = self.interner.intern(type_name);
            self.output.insert_node_kind_id(NodeKind::Named(sym), id);
        }
        if resolved.is_none() {
            let all_types = self.grammar.all_named_node_kinds();
            let max_dist = (type_name.len() / 3).clamp(2, 4);
            let suggestion = find_similar(type_name, &all_types, max_dist);

            let mut builder = self
                .diag
                .report(
                    located.source(),
                    DiagnosticKind::UnknownNodeKind,
                    type_token.text_range(),
                )
                .detail(type_name);

            if let Some(similar) = suggestion {
                builder = builder.fix(format!("did you mean `{}`?", similar), similar);
            }
            builder.emit();
        }
    }

    fn resolve_field_by_token(&mut self, source: SourceId, name_token: Option<SyntaxToken>) {
        let Some(name_token) = name_token else {
            return;
        };
        let field_name = name_token.text();
        if self.node_field_ids.contains_key(field_name) {
            return;
        }
        let resolved = self.grammar.resolve_field(field_name);
        self.node_field_ids
            .insert(token_src(&name_token, self.content(source)), resolved);
        if let Some(id) = resolved {
            let sym = self.interner.intern(field_name);
            self.output.insert_node_field_id(sym, id);
            return;
        }
        let all_fields = self.grammar.all_field_names();
        let max_dist = (field_name.len() / 3).clamp(2, 4);
        let suggestion = find_similar(field_name, &all_fields, max_dist);

        let mut builder = self
            .diag
            .report(
                source,
                DiagnosticKind::UnknownField,
                name_token.text_range(),
            )
            .detail(field_name);

        if let Some(similar) = suggestion {
            builder = builder.fix(format!("did you mean `{}`?", similar), similar);
        }
        builder.emit();
    }

    fn check_grammar(&mut self, source: SourceId, root: &ast::Root) {
        let defs: Vec<_> = root.defs().collect();
        for def in defs {
            let Some(body) = def.body() else { continue };
            let located = Located::new(source, body);
            let mut walk = RefCheckState::default();
            self.check_pattern_grammar(&located, None, GrammarCheckMode::Required, &mut walk);
        }
    }

    /// Walk the query, validating each node's own grammar constraints. See `GrammarCheckMode` for why
    /// `Deferred` positions skip their checks.
    ///
    /// The `Located` carries the source of the pattern being walked, so a reference
    /// into another workspace file is validated against the target's own source.
    fn check_pattern_grammar(
        &mut self,
        located: &Located<Pattern>,
        ctx: Option<ParentNodeCtx>,
        mode: GrammarCheckMode,
        walk: &mut RefCheckState,
    ) {
        match located.node() {
            Pattern::NodePattern(node) => {
                let located_node = located.wrap(node.clone());
                let child_ctx = self.resolve_node_context(&located_node);

                // A `#subtype` refinement must denote a satisfiable kind of its base type.
                if mode.is_required() {
                    self.validate_subtype(&located_node);
                }

                // Predicates are only valid on leaf nodes. Skipped under a disjunction/option,
                // where this position need not match for the query to.
                if mode.is_required()
                    && let Some(pred) = node.predicate()
                    && let Some(ctx) = &child_ctx
                    && (!self.grammar.valid_child_types(ctx.parent_id).is_empty()
                        || !self.grammar.fields_for_node_kind(ctx.parent_id).is_empty())
                {
                    self.diag
                        .report(
                            located.source(),
                            DiagnosticKind::PredicateOnNonLeaf,
                            pred.syntax().text_range(),
                        )
                        .emit();
                }

                let admissible = child_ctx
                    .as_ref()
                    .map(|ctx| self.admissible_set(ctx.parent_id));

                for child in node.children() {
                    if let Pattern::FieldPattern(f) = &child {
                        let located_field = located.wrap(f.clone());
                        self.validate_field_pattern(&located_field, child_ctx.as_ref(), mode, walk);
                    } else {
                        let child_located = located.wrap(child);
                        if mode.is_required()
                            && let (Some(ctx), Some(adm)) =
                                (child_ctx.as_ref(), admissible.as_ref())
                        {
                            self.check_bare_child(&child_located, ctx, adm);
                        }
                        self.check_pattern_grammar(&child_located, child_ctx, mode, walk);
                    }
                }

                if let Some(ctx) = child_ctx {
                    for child in node.syntax().children() {
                        if let Some(neg) = ast::NegatedField::cast(child) {
                            let located_neg = located.wrap(neg);
                            self.validate_negated_field(&located_neg, &ctx, mode);
                        }
                    }
                }
            }
            Pattern::TokenPattern(_) => {}
            Pattern::FieldPattern(f) => {
                // Normally handled by the parent NodePattern; reached only on a bare field
                // at root or inside a seq without a named-node parent.
                let located_field = located.wrap(f.clone());
                self.validate_field_pattern(&located_field, ctx.as_ref(), mode, walk);
            }
            Pattern::Union(_) | Pattern::Enum(_) => {
                // A branch is disjunctive — none is guaranteed to match, so defer its contents.
                for body in located.node().children() {
                    let body_located = located.wrap(body);
                    self.check_pattern_grammar(
                        &body_located,
                        ctx,
                        GrammarCheckMode::Deferred,
                        walk,
                    );
                }
            }
            Pattern::SeqPattern(seq) => {
                for child in seq.children() {
                    let child_located = located.wrap(child);
                    self.check_pattern_grammar(&child_located, ctx, mode, walk);
                }
            }
            Pattern::CapturedPattern(cap) => {
                let Some(inner) = cap.inner() else { return };
                let inner_located = located.wrap(inner);
                self.check_pattern_grammar(&inner_located, ctx, mode, walk);
            }
            Pattern::QuantifiedPattern(q) => {
                let Some(inner) = q.inner() else { return };
                // The body is optional/repeated — zero occurrences can satisfy it, so defer.
                let inner_located = located.wrap(inner);
                self.check_pattern_grammar(&inner_located, ctx, GrammarCheckMode::Deferred, walk);
            }
            Pattern::Ref(r) => {
                let Some(name_token) = r.name() else { return };
                let name = name_token.text();
                // Validation is a pure function of `(name, ctx, mode)`, so caching it
                // collapses diamond-shaped reference graphs that would otherwise be re-walked
                // 2^depth times. `mode` is part of the key: a definition reached both inside
                // and outside an alternation/quantifier must still be checked in its immediate
                // context even after the deferred reach cached it. Cut cycles are never cached:
                // they return below without reaching the `validated.insert`.
                let key = (name.to_string(), ctx, mode);
                if walk.validated.contains(&key) {
                    return;
                }
                if !walk.in_progress.insert(name.to_string()) {
                    return;
                }
                let Some(target) = self.symbol_table.located_definition(name) else {
                    walk.in_progress.remove(name);
                    return;
                };
                // The referenced definition may live in another workspace file; the
                // target carries its own source, so its body is validated against the
                // right content.
                self.check_pattern_grammar(&target, ctx, mode, walk);
                walk.in_progress.remove(name);
                walk.validated.insert(key);
            }
        }
    }

    fn resolve_node_context(&self, located: &Located<NodePattern>) -> Option<ParentNodeCtx> {
        let node = located.node();
        if node.is_any() {
            return None;
        }
        let type_token = node.kind_token()?;
        if matches!(
            type_token.kind(),
            SyntaxKind::KwError | SyntaxKind::KwMissing
        ) {
            return None;
        }
        let key = NodeKind::Named(token_src(&type_token, self.content(located.source())));
        let parent_id = self.node_kind_ids.get(&key).copied().flatten()?;
        self.grammar.node_kind(parent_id)?;
        Some(ParentNodeCtx {
            parent_id,
            parent_range: type_token.text_range(),
            parent_source: located.source(),
        })
    }

    fn validate_field_pattern(
        &mut self,
        located: &Located<ast::FieldPattern>,
        ctx: Option<&ParentNodeCtx>,
        mode: GrammarCheckMode,
        walk: &mut RefCheckState,
    ) {
        let field = located.node();
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
            if mode.is_required() {
                self.emit_field_not_on_node(
                    located.span_of(name_token.text_range()),
                    name_token.text(),
                    ctx,
                );
            }
            return;
        }

        let field_ref = FieldRef {
            id: field_id,
            name: name_token.text(),
            span: located.span_of(name_token.text_range()),
        };

        let Some(value) = field.value() else { return };
        let value_located = located.wrap(value);
        // The field value's kind must be admissible for this field. Skipped under a
        // disjunction/option, where the field constraint need not hold for the query to match.
        if mode.is_required() {
            self.check_field_value(&value_located, ctx, &field_ref);
        }
        self.check_pattern_grammar(&value_located, Some(*ctx), mode, walk);
    }

    fn validate_negated_field(
        &mut self,
        located: &Located<ast::NegatedField>,
        ctx: &ParentNodeCtx,
        mode: GrammarCheckMode,
    ) {
        let neg = located.node();
        let Some(name_token) = neg.name() else {
            return;
        };
        let field_name = name_token.text();

        let Some(field_id) = self.node_field_ids.get(field_name).copied().flatten() else {
            return;
        };

        if !self.grammar.has_field(ctx.parent_id, field_id) {
            if mode.is_required() {
                self.emit_field_not_on_node(
                    located.span_of(name_token.text_range()),
                    field_name,
                    ctx,
                );
            }
            return;
        }

        // A required field is present in every production, so asserting its absence can never
        // match. Skipped under a disjunction/option, where the negation need not hold.
        if mode.is_required()
            && self
                .grammar
                .field_cardinality(ctx.parent_id, field_id)
                .is_some_and(|cardinality| cardinality.is_required())
        {
            let parent_name = self
                .grammar
                .node_kind(ctx.parent_id)
                .expect("validated parent_id must have a name");
            self.diag
                .report(
                    located.source(),
                    DiagnosticKind::NegatedRequiredField,
                    name_token.text_range(),
                )
                .detail(field_name)
                .related_to(
                    ctx.parent_source,
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

    fn emit_field_not_on_node(&mut self, span: Span, field_name: &str, ctx: &ParentNodeCtx) {
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

    /// Resolve a child/value `NodePattern` to its grammar id, mirroring `resolve_node_context` but
    /// returning just the id. `None` for `(_)`, `ERROR`, `MISSING`, or an unresolved kind
    /// (the latter already reported by the resolution pass) — all of which carry no
    /// admissibility signal and are conservatively accepted.
    fn resolve_named_node_id(&self, located: &Located<NodePattern>) -> Option<NodeKindId> {
        let node = located.node();
        if node.is_any() {
            return None;
        }
        let type_token = node.kind_token()?;
        if matches!(
            type_token.kind(),
            SyntaxKind::KwError | SyntaxKind::KwMissing
        ) {
            return None;
        }
        let key = NodeKind::Named(token_src(&type_token, self.content(located.source())));
        self.node_kind_ids.get(&key).copied().flatten()
    }

    /// All child kinds (named children and field values) the grammar can place under `parent`,
    /// expanded through supertype subtyping in both directions. A bare child is admissible iff
    /// it lands in this set (or is an extra / a supertype overlapping it).
    fn admissible_set(&self, parent: NodeKindId) -> HashSet<NodeKindId> {
        let mut seeds = self.grammar.valid_child_types(parent).to_vec();
        for field_name in self.grammar.fields_for_node_kind(parent) {
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
    fn admissible_child(&self, child: NodeKindId, adm: &HashSet<NodeKindId>) -> bool {
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
        located: &Located<Pattern>,
        ctx: &ParentNodeCtx,
        adm: &HashSet<NodeKindId>,
    ) {
        match located.node() {
            Pattern::CapturedPattern(cap) => {
                if let Some(inner) = cap.inner() {
                    self.check_bare_child(&located.wrap(inner), ctx, adm);
                }
            }
            Pattern::SeqPattern(seq) => {
                for child in seq.children() {
                    self.check_bare_child(&located.wrap(child), ctx, adm);
                }
            }
            Pattern::NodePattern(node) => {
                self.check_bare_named_child(&located.wrap(node.clone()), ctx, adm);
            }
            // Anonymous children are untracked (grammar children arrays never list anonymous
            // tokens). Alternations, quantifiers, and references are not checked here.
            Pattern::TokenPattern(_)
            | Pattern::Union(_)
            | Pattern::Enum(_)
            | Pattern::QuantifiedPattern(_)
            | Pattern::Ref(_)
            | Pattern::FieldPattern(_) => {}
        }
    }

    fn check_bare_named_child(
        &mut self,
        located: &Located<NodePattern>,
        ctx: &ParentNodeCtx,
        adm: &HashSet<NodeKindId>,
    ) {
        let node = located.node();
        let parent_is_token = self.grammar.is_token(ctx.parent_id);

        // `(_)` matches any named node, so it is impossible only beneath a leaf token.
        if node.is_any() {
            if parent_is_token {
                self.emit_child_under_leaf_token(located.span_of(node.text_range()), ctx);
            }
            return;
        }

        let Some(child_id) = self.resolve_named_node_id(located) else {
            return;
        };
        let Some(type_token) = node.kind_token() else {
            return;
        };

        if parent_is_token {
            self.emit_child_under_leaf_token(located.span_of(type_token.text_range()), ctx);
            return;
        }

        if !self.admissible_child(child_id, adm) {
            self.emit_invalid_child(located.span_of(type_token.text_range()), child_id, ctx);
        }
    }

    /// Validate one field value against the field's admissible types. Mirrors `check_bare_child`
    /// but uses the field's type set and has no extras/leaf-token rescue (fields hold specific
    /// kinds, never comments).
    fn check_field_value(
        &mut self,
        located: &Located<Pattern>,
        ctx: &ParentNodeCtx,
        field: &FieldRef,
    ) {
        match located.node() {
            Pattern::CapturedPattern(cap) => {
                if let Some(inner) = cap.inner() {
                    self.check_field_value(&located.wrap(inner), ctx, field);
                }
            }
            Pattern::NodePattern(node) => {
                self.check_field_named_value(&located.wrap(node.clone()), ctx, field);
            }
            Pattern::TokenPattern(anon) => {
                self.check_field_anon_value(&located.wrap(anon.clone()), ctx, field);
            }
            // Alternations, quantifiers, and references are not checked here; a field value
            // can't be a sequence (rejected earlier as `FieldSequenceValue`).
            Pattern::Union(_)
            | Pattern::Enum(_)
            | Pattern::QuantifiedPattern(_)
            | Pattern::Ref(_)
            | Pattern::SeqPattern(_)
            | Pattern::FieldPattern(_) => {}
        }
    }

    fn check_field_named_value(
        &mut self,
        located: &Located<NodePattern>,
        ctx: &ParentNodeCtx,
        field: &FieldRef,
    ) {
        let node = located.node();
        if node.is_any() {
            // `(_)` matches any named node — impossible only when the field admits literal
            // tokens exclusively.
            if self.field_is_anonymous_only(ctx.parent_id, field.id) {
                let message = format!("a named node can't be the value of `{}`", field.name);
                self.emit_invalid_field_value(
                    located.span_of(node.text_range()),
                    message,
                    ctx,
                    field,
                );
            }
            return;
        }

        let Some(value_id) = self.resolve_named_node_id(located) else {
            return;
        };
        let Some(type_token) = node.kind_token() else {
            return;
        };

        if self.field_admissible(value_id, ctx.parent_id, field.id) {
            return;
        }

        let value_name = self
            .grammar
            .node_kind(value_id)
            .expect("resolved value must have a name");
        let message = format!("`{}` can't be the value of `{}`", value_name, field.name);
        self.emit_invalid_field_value(
            located.span_of(type_token.text_range()),
            message,
            ctx,
            field,
        );
    }

    fn check_field_anon_value(
        &mut self,
        located: &Located<ast::TokenPattern>,
        ctx: &ParentNodeCtx,
        field: &FieldRef,
    ) {
        let anon = located.node();
        // The bare `_` matches any node, anonymous tokens included, so it always fits.
        if anon.is_any() {
            return;
        }
        let Some(value_token) = anon.value() else {
            return;
        };
        let key = NodeKind::Anonymous(token_src(&value_token, self.content(located.source())));
        let Some(value_id) = self.node_kind_ids.get(&key).copied().flatten() else {
            return;
        };

        if self
            .field_admissible_set(ctx.parent_id, field.id)
            .contains(&value_id)
        {
            return;
        }

        let value_name = value_token.text().to_string();
        let message = format!("`{}` can't be the value of `{}`", value_name, field.name);
        self.emit_invalid_field_value(
            located.span_of(value_token.text_range()),
            message,
            ctx,
            field,
        );
    }

    fn field_admissible_set(
        &self,
        parent: NodeKindId,
        field_id: NodeFieldId,
    ) -> HashSet<NodeKindId> {
        let mut admissible = HashSet::new();
        for &seed in self.grammar.valid_field_types(parent, field_id) {
            admissible.insert(seed);
            admissible.extend(self.grammar.collect_subtypes(seed));
        }
        admissible
    }

    fn field_admissible(
        &self,
        value: NodeKindId,
        parent: NodeKindId,
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

    /// True only when every kind the field accepts is a literal token — then a `(_)` (named)
    /// value is impossible. Errs toward `false` (accept) when any kind isn't confidently
    /// anonymous, keeping rejection sound.
    fn field_is_anonymous_only(&self, parent: NodeKindId, field_id: NodeFieldId) -> bool {
        let types = self.grammar.valid_field_types(parent, field_id);
        !types.is_empty() && types.iter().all(|&id| self.grammar.is_anonymous_node(id))
    }

    /// Whether two kinds can denote the same concrete node — i.e. their subtype closures (each
    /// including the kind itself) intersect. Even sibling supertypes can share a concrete member,
    /// so a non-empty overlap is what makes a `(super#sub)` refinement satisfiable.
    fn subtypes_overlap(&self, a: NodeKindId, b: NodeKindId) -> bool {
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
    fn validate_subtype(&mut self, located: &Located<NodePattern>) {
        let node = located.node();
        let Some(sub_token) = node.subtype() else {
            return;
        };
        let Some(super_token) = node.kind_token() else {
            return;
        };
        if matches!(
            super_token.kind(),
            SyntaxKind::Underscore | SyntaxKind::KwError | SyntaxKind::KwMissing
        ) {
            return;
        }
        let Some(super_id) = self.resolve_named_node_id(located) else {
            return;
        };

        let sub_name = sub_token.text();
        let Some(sub_id) = self.grammar.resolve_named_node(sub_name) else {
            let all_types = self.grammar.all_named_node_kinds();
            let max_dist = (sub_name.len() / 3).clamp(2, 4);
            let suggestion = find_similar(sub_name, &all_types, max_dist).map(str::to_string);
            let mut builder = self
                .diag
                .report(
                    located.source(),
                    DiagnosticKind::UnknownNodeKind,
                    sub_token.text_range(),
                )
                .detail(sub_name);
            if let Some(similar) = suggestion {
                builder = builder.fix(format!("did you mean `{}`?", similar), similar);
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
            .node_kind(super_id)
            .expect("resolved supertype must have a name")
            .to_string();
        let kinds = self
            .grammar
            .subtypes(super_id)
            .iter()
            .filter_map(|&id| self.grammar.node_kind(id))
            .collect::<Vec<_>>();
        let kinds_hint = (!kinds.is_empty()).then(|| {
            format!(
                "subtypes of `{}` include: {}",
                super_name,
                format_list(&kinds, 8)
            )
        });

        let mut builder = self
            .diag
            .report(
                located.source(),
                DiagnosticKind::InvalidSubtype,
                sub_token.text_range(),
            )
            .detail(format!(
                "`{}` is not a subtype of `{}`",
                sub_name, super_name
            ))
            .related_to(
                located.source(),
                super_token.text_range(),
                format!("base type `{}`", super_name),
            );
        if let Some(hint) = kinds_hint {
            builder = builder.hint(hint);
        }
        builder.emit();
    }

    fn emit_invalid_child(&mut self, span: Span, child_id: NodeKindId, ctx: &ParentNodeCtx) {
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

    fn emit_child_under_leaf_token(&mut self, span: Span, ctx: &ParentNodeCtx) {
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

    fn emit_invalid_field_value(
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

/// Whether structural checks must fire at this position. Set to `Deferred` once the
/// walk descends into an alternation branch or a quantified body: inside those, nothing is
/// guaranteed to participate in a match (a sibling branch or zero repetitions can satisfy the
/// query), so the grammar checks must NOT fire there — doing so would reject queries that can
/// match. Skipping a check can only miss a rejection, never reject a valid query.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum GrammarCheckMode {
    Required,
    Deferred,
}

impl GrammarCheckMode {
    fn is_required(self) -> bool {
        matches!(self, GrammarCheckMode::Required)
    }
}

struct FieldRef<'a> {
    id: NodeFieldId,
    name: &'a str,
    span: Span,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct ParentNodeCtx {
    parent_id: NodeKindId,
    parent_range: TextRange,
    /// Source the parent node lives in. May differ from the source currently
    /// being walked once validation crosses a reference into another workspace
    /// file, so `parent_range` must be reported against this, not the active source.
    parent_source: SourceId,
}

#[derive(Default)]
struct RefCheckState {
    /// Definitions currently on the recursion stack — guards against cycles.
    in_progress: HashSet<String>,
    /// Definitions already validated under a given context. A definition's
    /// validation depends only on `(name, ctx, mode)`, so caching it keeps shared
    /// references (e.g. diamond graphs) from being re-walked exponentially.
    validated: HashSet<(String, Option<ParentNodeCtx>, GrammarCheckMode)>,
}

struct SymbolResolver<'l, 'a, 'q> {
    linker: &'l mut GrammarLinker<'a, 'q>,
}

impl Visitor for SymbolResolver<'_, '_, '_> {
    fn visit(&mut self, root: &Located<ast::Root>) {
        walk(self, root);
    }

    fn visit_node_pattern(&mut self, node: &Located<ast::NodePattern>) {
        self.linker.resolve_named_node(node);

        for neg in node
            .node()
            .syntax()
            .children()
            .filter_map(ast::NegatedField::cast)
        {
            self.linker
                .resolve_field_by_token(node.source(), neg.name());
        }

        crate::compiler::analyze::visitor::walk_node_pattern(self, node);
    }

    fn visit_token_pattern(&mut self, node: &Located<ast::TokenPattern>) {
        let home = node.source();
        let token = node.node();
        if token.is_any() {
            return;
        }
        let Some(value_token) = token.value() else {
            return;
        };
        let value = value_token.text();
        let key = NodeKind::Anonymous(token_src(&value_token, self.linker.content(home)));
        if self.linker.node_kind_ids.contains_key(&key) {
            return;
        }

        let resolved = self.linker.grammar.resolve_anonymous_node(value);
        self.linker.node_kind_ids.insert(key, resolved);

        if let Some(id) = resolved {
            let sym = self.linker.interner.intern(value);
            self.linker
                .output
                .insert_node_kind_id(NodeKind::Anonymous(sym), id);
            return;
        }

        self.linker
            .diag
            .report(
                home,
                DiagnosticKind::UnknownNodeKind,
                value_token.text_range(),
            )
            .detail(value)
            .emit();
    }

    fn visit_field_pattern(&mut self, field: &Located<ast::FieldPattern>) {
        self.linker
            .resolve_field_by_token(field.source(), field.node().name());
        crate::compiler::analyze::visitor::walk_field_pattern(self, field);
    }
}
