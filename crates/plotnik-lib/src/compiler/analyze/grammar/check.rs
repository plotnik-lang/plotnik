use std::collections::{HashMap, HashSet};

use crate::compiler::analyze::Located;
use crate::compiler::diagnostics::report::{DiagnosticKind, Span};
use crate::compiler::ids::DefId;
use crate::compiler::parse::ast::token_src;
use crate::compiler::parse::ast::{self, NamedNodePattern, Pattern};
use crate::compiler::parse::cst::SyntaxKind;
use crate::core::grammar::Grammar;
use crate::core::{NodeFieldId, NodeKind, NodeKindId};

use super::bind::GrammarBinder;
use super::diagnostics::format_list;
use super::participation::Participation;

impl<'a, 'q> GrammarBinder<'a, 'q> {
    /// Walk the query, validating each node's own grammar constraints. See
    /// [`Participation`] for why deferred positions skip their checks.
    ///
    /// The `Located` carries the source of the pattern being walked, so a reference
    /// into another workspace file is validated against the target's own source.
    pub(super) fn check_pattern_grammar(
        &mut self,
        located: &Located<Pattern>,
        ctx: Option<ParentNode>,
        participation: Participation,
        walk: &mut AdmissibilityWalkState,
    ) {
        match located.node() {
            Pattern::NamedNodePattern(node) => {
                let located_node = located.wrap(node.clone());
                // The VM only matches concrete tree-sitter node kinds today. Stop here so
                // this unsupported supertype does not become context for child checks.
                if self.reject_supertype_match(&located_node) {
                    return;
                }
                let child_ctx = self.resolve_node_context(&located_node);

                if let Some(predicate) = node.predicate()
                    && let Some(operator) = predicate.operator()
                {
                    let mismatched = (operator.is_regex_op() && predicate.string_value().is_some())
                        || (!operator.is_regex_op() && predicate.regex().is_some());
                    if mismatched {
                        let node_kind = node
                            .kind_token()
                            .expect("validated named-node pattern has a kind");
                        let node_name = node_kind.text();
                        let operator_token = predicate
                            .operator_token()
                            .expect("resolved predicate operator has a token");
                        let operator_text = operator_token.text();
                        let predicate_text = predicate.syntax().text();
                        let (required, supplied) = if operator.is_regex_op() {
                            ("a regex value", "a quoted string")
                        } else {
                            ("a quoted string", "a regex value")
                        };
                        let mut builder = self
                            .diag
                            .report(
                                DiagnosticKind::PredicateValueMismatch,
                                located.span_of(predicate.syntax().text_range()),
                            )
                            .detail(format!(
                                "predicate `{node_name} {predicate_text}` uses `{operator_text}`, which requires {required}, but supplies {supplied}"
                            ))
                            .hint(format!(
                                "change the value in `{predicate_text}` to {required}, or choose an operator that accepts {supplied}"
                            ));
                        if let Some(ctx) = &child_ctx {
                            builder = builder.related_to(
                                ctx.span(),
                                format!("predicate applies to `{node_name}`"),
                            );
                        }
                        builder.emit();
                    }
                }

                // Predicates are only valid on leaf nodes. Skipped under a disjunction/option,
                // where this position need not match for the query to.
                if participation.is_required()
                    && let Some(pred) = node.predicate()
                    && let Some(ctx) = &child_ctx
                    && self.grammar.has_declared_child_structure(ctx.id())
                {
                    let node_name = ctx.name(self.grammar);
                    let predicate_text = pred.syntax().text();
                    self.diag
                        .report(
                            DiagnosticKind::PredicateOnNonLeaf,
                            located.span_of(pred.syntax().text_range()),
                        )
                        .detail(format!(
                            "predicate `{node_name} {predicate_text}` cannot test `{node_name}` because this node kind can contain children"
                        ))
                        .related_to(ctx.span(), format!("`{node_name}` starts here"))
                        .hint(format!(
                            "move `{predicate_text}` to a leaf child of `{node_name}`, or match a literal token directly"
                        ))
                        .emit();
                }

                for child in node.children() {
                    if let Pattern::FieldPattern(f) = &child {
                        let located_field = located.wrap(f.clone());
                        self.validate_field_pattern(
                            &located_field,
                            child_ctx.as_ref(),
                            participation,
                            walk,
                        );
                    } else {
                        let child_located = located.wrap(child);
                        if participation.is_required()
                            && let Some(ctx) = child_ctx.as_ref()
                        {
                            let adm = walk.admissible_set(self.grammar, ctx.id());
                            self.check_bare_child(&child_located, ctx, adm);
                        }
                        self.check_pattern_grammar(&child_located, child_ctx, participation, walk);
                    }
                }

                if let Some(ctx) = child_ctx {
                    for child in node.syntax().children() {
                        if let Some(neg) = ast::NegatedField::cast(child) {
                            let located_neg = located.wrap(neg);
                            self.validate_negated_field(&located_neg, &ctx, participation);
                        }
                    }
                }
            }
            Pattern::AnonymousNodePattern(_) | Pattern::NodeWildcard(_) => {}
            Pattern::FieldPattern(f) => {
                // Normally handled by the parent named-node pattern; reached only on a bare field
                // at root or inside a seq without a named-node parent.
                let located_field = located.wrap(f.clone());
                self.validate_field_pattern(&located_field, ctx.as_ref(), participation, walk);
            }
            Pattern::Alternation(_) => {
                // An alternative is disjunctive — none is guaranteed to match, so defer its contents.
                for body in located.node().children() {
                    let body_located = located.wrap(body);
                    self.check_pattern_grammar(
                        &body_located,
                        ctx,
                        participation.inside_alternative(),
                        walk,
                    );
                }
            }
            Pattern::SeqPattern(seq) => {
                for child in seq.children() {
                    let child_located = located.wrap(child);
                    self.check_pattern_grammar(&child_located, ctx, participation, walk);
                }
            }
            Pattern::CapturedPattern(cap) => {
                let inner = cap
                    .inner()
                    .expect("validated captured pattern has an inner pattern");
                let inner_located = located.wrap(inner);
                self.check_pattern_grammar(&inner_located, ctx, participation, walk);
            }
            Pattern::QuantifiedPattern(q) => {
                let inner = q
                    .inner()
                    .expect("validated quantified pattern has an inner pattern");
                let inner_participation = participation.inside_quantifier_body(q);
                let inner_located = located.wrap(inner);
                self.check_pattern_grammar(&inner_located, ctx, inner_participation, walk);
            }
            Pattern::DefRef(r) => {
                let def_id = self
                    .definitions
                    .reference_target(r)
                    .expect("admitted definition reference must resolve");
                // Validation is a pure function of `(definition, ctx, participation)`, so caching it
                // collapses diamond-shaped reference graphs that would otherwise be re-walked
                // 2^depth times. `participation` is part of the key: a definition reached
                // both inside and outside an alternation/quantifier must still be checked in
                // its immediate context even after the deferred reach cached it. Cut cycles
                // are never cached: they return below without reaching the `validated.insert`.
                let key = (def_id, ctx, participation);
                if walk.validated.contains(&key) {
                    return;
                }
                if !walk.in_progress.insert(def_id) {
                    return;
                }
                let target = self.definitions.definition(def_id).located_body();
                // The referenced definition may live in another workspace file; the
                // target carries its own source, so its body is validated against the
                // right content.
                self.check_pattern_grammar(&target, ctx, participation, walk);
                walk.in_progress.remove(&def_id);
                walk.validated.insert(key);
            }
        }
    }

    /// Conservative entry point root admissibility: return `false` only when the
    /// definition's outermost node-consuming pattern is known not to be the grammar root.
    pub(super) fn pattern_can_match_root(
        &self,
        located: &Located<Pattern>,
        grammar_root: NodeKindId,
        seen_refs: &mut HashSet<DefId>,
    ) -> bool {
        match located.node() {
            Pattern::NamedNodePattern(node) => {
                self.node_pattern_can_match_root(&located.wrap(node.clone()), grammar_root)
            }
            Pattern::AnonymousNodePattern(_) => false,
            Pattern::NodeWildcard(_) => true,
            Pattern::CapturedPattern(cap) => {
                let Some(inner) = cap.inner() else {
                    return true;
                };
                self.pattern_can_match_root(&located.wrap(inner), grammar_root, seen_refs)
            }
            Pattern::QuantifiedPattern(q) => {
                if q.is_optional() {
                    return true;
                }
                let Some(inner) = q.inner() else {
                    return true;
                };
                self.pattern_can_match_root(&located.wrap(inner), grammar_root, seen_refs)
            }
            Pattern::Alternation(alternation) => {
                let mut saw_alternative = false;
                for alternative in alternation.alternatives() {
                    saw_alternative = true;
                    if alternative.body().is_none_or(|body| {
                        self.pattern_can_match_root(&located.wrap(body), grammar_root, seen_refs)
                    }) {
                        return true;
                    }
                }
                !saw_alternative
            }
            Pattern::DefRef(def_ref) => {
                let Some(def_id) = self.definitions.reference_target(def_ref) else {
                    return true;
                };
                if !seen_refs.insert(def_id) {
                    return true;
                }
                let target = self.definitions.definition(def_id).located_body();
                let result = self.pattern_can_match_root(&target, grammar_root, seen_refs);
                seen_refs.remove(&def_id);
                result
            }
            Pattern::SeqPattern(seq) => {
                let mut children = seq.children();
                let Some(first) = children.next() else {
                    return true;
                };
                if children.next().is_some() {
                    return true;
                }
                self.pattern_can_match_root(&located.wrap(first), grammar_root, seen_refs)
            }
            // Bare fields are already reported by grammar validation; avoid piling a
            // entry point root warning onto malformed structure.
            Pattern::FieldPattern(_) => true,
        }
    }

    fn node_pattern_can_match_root(
        &self,
        located: &Located<NamedNodePattern>,
        grammar_root: NodeKindId,
    ) -> bool {
        if located.node().is_any() {
            return true;
        }
        let Some(id) = self.resolve_named_node_id(located) else {
            return true;
        };
        if self.grammar.is_supertype(id) {
            return true;
        }
        id == grammar_root
    }

    fn resolve_node_context(&self, located: &Located<NamedNodePattern>) -> Option<ParentNode> {
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
        participation: Participation,
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
            // A field absent from this kind can never match here, but a sibling alternative or zero
            // repetitions can — so skip when deferred.
            if participation.is_required() {
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
        if participation.is_required() {
            self.check_field_value(&value_located, ctx, &field_ref, walk);
        }
        self.check_pattern_grammar(&value_located, Some(*ctx), participation, walk);
    }

    fn validate_negated_field(
        &mut self,
        located: &Located<ast::NegatedField>,
        ctx: &ParentNode,
        participation: Participation,
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
            if participation.is_required() {
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
        if !participation.is_required() {
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
                "`-{0}` requires `{0}` to be absent, but every `{1}` has one. Drop `-{0}`",
                field_name, parent_name
            ))
            .emit();
    }

    /// Whether a concrete child kind can occupy a bare child position whose parent admits
    /// `adm`. The parent is already known to be a non-leaf here.
    fn admissible_child(&self, child: NodeKindId, adm: &HashSet<NodeKindId>) -> bool {
        adm.contains(&child)
            // Tolerated over-acceptance: an extra (a comment) is admitted under *any*
            // parent, even a lexically sealed node like `string` that never holds one, so
            // `(string (comment))` slips through. Proving a position sealed is a lexer-level
            // question (token longest-match/precedence), not the `IMMEDIATE_TOKEN` fact it
            // resembles — closing delimiters are non-immediate in nearly every grammar — and
            // our metadata-only model can't answer it. Sound: this only widens admissibility.
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
            Pattern::NamedNodePattern(node) => {
                self.check_bare_named_child(&located.wrap(node.clone()), ctx, adm);
            }
            // Anonymous children are untracked (grammar children arrays never list anonymous
            // tokens). Alternations, quantifiers, and references are not checked here.
            Pattern::AnonymousNodePattern(_)
            | Pattern::NodeWildcard(_)
            | Pattern::Alternation(_)
            | Pattern::QuantifiedPattern(_)
            | Pattern::DefRef(_)
            | Pattern::FieldPattern(_) => {}
        }
    }

    fn check_bare_named_child(
        &mut self,
        located: &Located<NamedNodePattern>,
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
        walk: &mut AdmissibilityWalkState,
    ) {
        match located.node() {
            Pattern::CapturedPattern(cap) => {
                if let Some(inner) = cap.inner() {
                    self.check_field_value(&located.wrap(inner), ctx, field, walk);
                }
            }
            Pattern::NamedNodePattern(node) => {
                self.check_field_named_value(&located.wrap(node.clone()), ctx, field, walk);
            }
            Pattern::AnonymousNodePattern(anon) => {
                self.check_field_anon_value(&located.wrap(anon.clone()), ctx, field, walk);
            }
            Pattern::NodeWildcard(_) => {}
            // Alternations, quantifiers, and references are not checked here; a field value
            // can't be a sequence (rejected earlier as `GrammarFieldSequenceValue`).
            Pattern::Alternation(_)
            | Pattern::QuantifiedPattern(_)
            | Pattern::DefRef(_)
            | Pattern::SeqPattern(_)
            | Pattern::FieldPattern(_) => {}
        }
    }

    fn check_field_named_value(
        &mut self,
        located: &Located<NamedNodePattern>,
        ctx: &ParentNode,
        field: &FieldRef,
        walk: &mut AdmissibilityWalkState,
    ) {
        let node = located.node();
        if node.is_any() {
            // `(_)` matches any named node — impossible only when the field admits literal
            // tokens exclusively.
            if self.field_is_anonymous_only(ctx.id(), field.id) {
                self.emit_invalid_field_value(
                    located.span_of(node.text_range()),
                    "a named node",
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

        let admissible = walk.field_admissible_set(self.grammar, ctx.id(), field.id);
        if self.field_admissible(value_id, admissible) {
            return;
        }

        let value_name = self
            .grammar
            .node_kind(value_id)
            .expect("resolved value must have a name");
        let value = format!("`{value_name}`");
        self.emit_invalid_field_value(located.span_of(type_token.text_range()), &value, ctx, field);
    }

    fn check_field_anon_value(
        &mut self,
        located: &Located<ast::AnonymousNodePattern>,
        ctx: &ParentNode,
        field: &FieldRef,
        walk: &mut AdmissibilityWalkState,
    ) {
        let anon = located.node();
        let Some(value_token) = anon.value() else {
            return;
        };
        let key = NodeKind::Anonymous(token_src(&value_token, self.content(located.source())));
        let Some(value_id) = self.node_kind_ids.get(&key).copied().flatten() else {
            return;
        };

        if walk
            .field_admissible_set(self.grammar, ctx.id(), field.id)
            .contains(&value_id)
        {
            return;
        }

        let value = format!("`{}`", value_token.text());
        self.emit_invalid_field_value(
            located.span_of(value_token.text_range()),
            &value,
            ctx,
            field,
        );
    }

    fn field_admissible(&self, value: NodeKindId, admissible: &HashSet<NodeKindId>) -> bool {
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
    fn reject_supertype_match(&mut self, located: &Located<NamedNodePattern>) -> bool {
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

        let kind = if node.has_supertype_marker() {
            DiagnosticKind::UnsupportedSupertype
        } else {
            DiagnosticKind::BareSupertype
        };
        let subtypes = self
            .grammar
            .subtypes(id)
            .iter()
            .filter_map(|&sub| self.grammar.node_kind(sub))
            .collect::<Vec<_>>();
        let mut builder = self.diag.report(kind, span).detail(name.as_str());
        if !subtypes.is_empty() {
            if subtypes.len() <= 8 {
                let alternatives = subtypes
                    .iter()
                    .map(|subtype| format!("({subtype})"))
                    .collect::<Vec<_>>()
                    .join(" ");
                builder = builder.hint(format!(
                    "replace `{name}` with an exhaustive alternation of its concrete subtypes: `[{alternatives}]`"
                ));
            } else {
                builder = builder.hint(format!(
                    "choose the concrete subtypes of `{name}` that this query should match"
                ));
                builder = builder.hint(format!(
                    "the grammar includes {}",
                    format_list(&subtypes, 5)
                ));
            }
        }
        builder.emit();
        true
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
    in_progress: HashSet<DefId>,
    /// Definitions already validated under a given context. A definition's
    /// validation depends only on `(DefId, ctx, participation)`, so caching it keeps shared
    /// references (e.g. diamond graphs) from being re-walked exponentially.
    validated: HashSet<(DefId, Option<ParentNode>, Participation)>,
    /// Expanded bare-child admissibility sets by parent node kind.
    admissible_by_parent: HashMap<NodeKindId, HashSet<NodeKindId>>,
    /// Expanded field-value admissibility sets by `(parent, field)`.
    admissible_by_field: HashMap<(NodeKindId, NodeFieldId), HashSet<NodeKindId>>,
}

impl AdmissibilityWalkState {
    /// All child kinds (named children and field values) the grammar can place under `parent`,
    /// expanded through supertype subtyping. A bare child is admissible iff it lands in this
    /// set (or is an extra / a supertype overlapping it).
    fn admissible_set(&mut self, grammar: &Grammar, parent: NodeKindId) -> &HashSet<NodeKindId> {
        self.admissible_by_parent.entry(parent).or_insert_with(|| {
            let seeds = grammar.valid_child_types(parent).iter().copied().chain(
                grammar
                    .field_ids_for_node_kind(parent)
                    .iter()
                    .flat_map(|&field| grammar.valid_field_types(parent, field).iter().copied()),
            );
            expanded_types(grammar, seeds)
        })
    }

    fn field_admissible_set(
        &mut self,
        grammar: &Grammar,
        parent: NodeKindId,
        field: NodeFieldId,
    ) -> &HashSet<NodeKindId> {
        let key = (parent, field);
        self.admissible_by_field.entry(key).or_insert_with(|| {
            let seeds = grammar.valid_field_types(parent, field).iter().copied();
            expanded_types(grammar, seeds)
        })
    }
}

fn expanded_types(
    grammar: &Grammar,
    seeds: impl IntoIterator<Item = NodeKindId>,
) -> HashSet<NodeKindId> {
    let mut admissible = HashSet::new();
    for seed in seeds {
        admissible.insert(seed);
        admissible.extend(grammar.collect_subtypes(seed));
    }
    admissible
}
