use std::collections::HashSet;

use crate::compiler::analyze::Located;
use crate::compiler::diagnostics::diagnostics::{DiagnosticKind, Span};
use crate::compiler::diagnostics::source::SourceId;
use crate::compiler::parse::ast::token_src;
use crate::compiler::parse::ast::{self, NodePattern, Pattern};
use crate::compiler::parse::cst::SyntaxKind;
use crate::core::{NodeFieldId, NodeKind, NodeKindId};
use rowan::TextRange;

use super::diagnostics::format_list;
use super::link::GrammarLinker;
use super::utils::find_similar;

impl<'a, 'q> GrammarLinker<'a, 'q> {
    /// Walk the query, validating each node's own grammar constraints. See `GrammarCheckMode` for why
    /// `Deferred` positions skip their checks.
    ///
    /// The `Located` carries the source of the pattern being walked, so a reference
    /// into another workspace file is validated against the target's own source.
    pub(super) fn check_pattern_grammar(
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
}

/// Whether structural checks must fire at this position. Set to `Deferred` once the
/// walk descends into an alternation branch or a quantified body: inside those, nothing is
/// guaranteed to participate in a match (a sibling branch or zero repetitions can satisfy the
/// query), so the grammar checks must NOT fire there — doing so would reject queries that can
/// match. Skipping a check can only miss a rejection, never reject a valid query.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum GrammarCheckMode {
    Required,
    Deferred,
}

impl GrammarCheckMode {
    fn is_required(self) -> bool {
        matches!(self, GrammarCheckMode::Required)
    }
}

pub(super) struct FieldRef<'a> {
    pub(super) id: NodeFieldId,
    pub(super) name: &'a str,
    pub(super) span: Span,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct ParentNodeCtx {
    pub(super) parent_id: NodeKindId,
    pub(super) parent_range: TextRange,
    /// Source the parent node lives in. May differ from the source currently
    /// being walked once validation crosses a reference into another workspace
    /// file, so `parent_range` must be reported against this, not the active source.
    pub(super) parent_source: SourceId,
}

#[derive(Default)]
pub(super) struct RefCheckState {
    /// Definitions currently on the recursion stack — guards against cycles.
    in_progress: HashSet<String>,
    /// Definitions already validated under a given context. A definition's
    /// validation depends only on `(name, ctx, mode)`, so caching it keeps shared
    /// references (e.g. diamond graphs) from being re-walked exponentially.
    validated: HashSet<(String, Option<ParentNodeCtx>, GrammarCheckMode)>,
}
