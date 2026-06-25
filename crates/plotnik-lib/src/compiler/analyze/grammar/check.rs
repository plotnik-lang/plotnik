use std::collections::HashSet;

use crate::compiler::analyze::Located;
use crate::compiler::diagnostics::report::{DiagnosticKind, Span};
use crate::compiler::parse::ast::token_src;
use crate::compiler::parse::ast::{self, NodePattern, Pattern};
use crate::compiler::parse::cst::SyntaxKind;
use crate::core::grammar::Grammar;
use crate::core::{NodeFieldId, NodeKind, NodeKindId};

use super::diagnostics::format_list;
use super::link::GrammarLinker;

impl<'a, 'q> GrammarLinker<'a, 'q> {
    /// Walk the query, validating each node's own grammar constraints. See `AdmissibilityMode` for why
    /// `Deferred` positions skip their checks.
    ///
    /// The `Located` carries the source of the pattern being walked, so a reference
    /// into another workspace file is validated against the target's own source.
    pub(super) fn check_pattern_grammar(
        &mut self,
        located: &Located<Pattern>,
        ctx: Option<ParentNode>,
        mode: AdmissibilityMode,
        walk: &mut AdmissibilityWalkState,
    ) {
        match located.node() {
            Pattern::NodePattern(node) => {
                let located_node = located.wrap(node.clone());
                // The VM only matches concrete tree-sitter node kinds today. Stop here so
                // this unsupported supertype does not become context for child checks.
                if self.reject_supertype_match(&located_node) {
                    return;
                }
                let child_ctx = self.resolve_node_context(&located_node);

                // Predicates are only valid on leaf nodes. Skipped under a disjunction/option,
                // where this position need not match for the query to.
                if mode.is_required()
                    && let Some(pred) = node.predicate()
                    && let Some(ctx) = &child_ctx
                    && (!self.grammar.valid_child_types(ctx.id()).is_empty()
                        || !self.grammar.fields_for_node_kind(ctx.id()).is_empty())
                {
                    self.diag
                        .report(
                            DiagnosticKind::PredicateOnNonLeaf,
                            located.span_of(pred.syntax().text_range()),
                        )
                        .emit();
                }

                let admissible = child_ctx.as_ref().map(|ctx| self.admissible_set(ctx.id()));

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
                        AdmissibilityMode::Deferred,
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
                self.check_pattern_grammar(&inner_located, ctx, AdmissibilityMode::Deferred, walk);
            }
            Pattern::DefRef(r) => {
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

    fn resolve_node_context(&self, located: &Located<NodePattern>) -> Option<ParentNode> {
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
        Some(ParentNode {
            id: parent_id,
            span: located.span_of(type_token.text_range()),
        })
    }

    fn validate_field_pattern(
        &mut self,
        located: &Located<ast::FieldPattern>,
        ctx: Option<&ParentNode>,
        mode: AdmissibilityMode,
        walk: &mut AdmissibilityWalkState,
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

        if !self.grammar.has_field(ctx.id(), field_id) {
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
        ctx: &ParentNode,
        mode: AdmissibilityMode,
    ) {
        let neg = located.node();
        let Some(name_token) = neg.name() else {
            return;
        };
        let field_name = name_token.text();

        let Some(field_id) = self.node_field_ids.get(field_name).copied().flatten() else {
            return;
        };

        if !self.grammar.has_field(ctx.id(), field_id) {
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
        if !mode.is_required() {
            return;
        }

        if !self
            .grammar
            .field_cardinality(ctx.id(), field_id)
            .is_some_and(|cardinality| cardinality.is_required())
        {
            return;
        }

        let parent_name = ctx.name(self.grammar);
        let parent_span = ctx.span();
        self.diag
            .report(
                DiagnosticKind::NegatedRequiredField,
                located.span_of(name_token.text_range()),
            )
            .detail(field_name)
            .related_to(parent_span, format!("on `{}`", parent_name))
            .hint(format!(
                "`-{0}` requires `{0}` to be absent, but every `{1}` has one — drop `-{0}`",
                field_name, parent_name
            ))
            .emit();
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
            admissible.extend(self.kind_with_subtypes(seed));
        }
        admissible
    }

    fn kind_with_subtypes(&self, kind: NodeKindId) -> HashSet<NodeKindId> {
        let mut kinds = self.grammar.collect_subtypes(kind);
        kinds.insert(kind);
        kinds
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
        ctx: &ParentNode,
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
            | Pattern::DefRef(_)
            | Pattern::FieldPattern(_) => {}
        }
    }

    fn check_bare_named_child(
        &mut self,
        located: &Located<NodePattern>,
        ctx: &ParentNode,
        adm: &HashSet<NodeKindId>,
    ) {
        let node = located.node();
        let parent_is_token = self.grammar.is_token(ctx.id());

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
        ctx: &ParentNode,
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
            | Pattern::DefRef(_)
            | Pattern::SeqPattern(_)
            | Pattern::FieldPattern(_) => {}
        }
    }

    fn check_field_named_value(
        &mut self,
        located: &Located<NodePattern>,
        ctx: &ParentNode,
        field: &FieldRef,
    ) {
        let node = located.node();
        if node.is_any() {
            // `(_)` matches any named node — impossible only when the field admits literal
            // tokens exclusively.
            if self.field_is_anonymous_only(ctx.id(), field.id) {
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

        if self.field_admissible(value_id, ctx.id(), field.id) {
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
        ctx: &ParentNode,
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
            .field_admissible_set(ctx.id(), field.id)
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

    /// Reject supertype patterns before lowering can emit a concrete-kind match that never fires.
    /// Bare `(expression)` gets a syntax-oriented diagnostic; marked `(expression#...)` gets the
    /// unsupported-feature diagnostic.
    /// Returns whether the node was rejected.
    fn reject_supertype_match(&mut self, located: &Located<NodePattern>) -> bool {
        let node = located.node();
        let Some(kind_token) = node.kind_token() else {
            return false;
        };
        let Some(id) = self.resolve_named_node_id(located) else {
            return false;
        };
        if !self.grammar.is_supertype(id) {
            return false;
        }

        let name = self
            .grammar
            .node_kind(id)
            .expect("resolved supertype must have a name")
            .to_string();
        let span = located.span_of(kind_token.text_range());

        if node.has_supertype_marker() {
            let subtypes = self
                .grammar
                .subtypes(id)
                .iter()
                .filter_map(|&sub| self.grammar.node_kind(sub))
                .collect::<Vec<_>>();
            let mut builder = self
                .diag
                .report(DiagnosticKind::UnsupportedSupertype, span)
                .detail(name.as_str());
            if !subtypes.is_empty() {
                builder = builder.hint(format!(
                    "subtypes of `{name}`: {}",
                    format_list(&subtypes, 8)
                ));
            }
            builder.emit();
        } else {
            self.diag
                .report(DiagnosticKind::BareSupertype, span)
                .detail(name.as_str())
                .hint(format!(
                    "use `({name}#)` instead, but it's not supported yet"
                ))
                .emit();
        }
        true
    }
}

/// Whether structural checks must fire at this position. Set to `Deferred` once the
/// walk descends into an alternation branch or a quantified body: inside those, nothing is
/// guaranteed to participate in a match (a sibling branch or zero repetitions can satisfy the
/// query), so the grammar checks must NOT fire there — doing so would reject queries that can
/// match. Skipping a check can only miss a rejection, never reject a valid query.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum AdmissibilityMode {
    Required,
    Deferred,
}

impl AdmissibilityMode {
    fn is_required(self) -> bool {
        matches!(self, AdmissibilityMode::Required)
    }
}

pub(super) struct FieldRef<'a> {
    pub(super) id: NodeFieldId,
    pub(super) name: &'a str,
    pub(super) span: Span,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct ParentNode {
    id: NodeKindId,
    /// Source the parent node lives in. May differ from the source currently
    /// being walked once validation crosses a reference into another workspace
    /// file, so this span must be reported against its own source.
    span: Span,
}

impl ParentNode {
    pub(super) fn id(self) -> NodeKindId {
        self.id
    }

    pub(super) fn span(self) -> Span {
        self.span
    }

    pub(super) fn name(self, grammar: &Grammar) -> &str {
        grammar
            .node_kind(self.id)
            .expect("validated parent node must have a name")
    }
}

#[derive(Default)]
pub(super) struct AdmissibilityWalkState {
    /// Definitions currently on the recursion stack — guards against cycles.
    in_progress: HashSet<String>,
    /// Definitions already validated under a given context. A definition's
    /// validation depends only on `(name, ctx, mode)`, so caching it keeps shared
    /// references (e.g. diamond graphs) from being re-walked exponentially.
    validated: HashSet<(String, Option<ParentNode>, AdmissibilityMode)>,
}
