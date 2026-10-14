use std::collections::HashSet;

use crate::compiler::analyze::Located;
use crate::compiler::analyze::visitor::{self, Visitor};
use crate::compiler::diagnostics::report::DiagnosticKind;
use crate::compiler::ids::DefId;
use crate::compiler::parse::ast::{self, NamedNodePattern, Pattern};
use crate::core::NodeKindId;

use super::bind::GrammarBinder;
use super::format::format_list;

impl GrammarBinder<'_, '_> {
    pub(super) fn validate_syntax(&mut self, pattern: &Located<Pattern>) {
        SyntaxValidator { binder: self }.visit_pattern(pattern);
    }

    fn validate_predicate(&mut self, located: &Located<NamedNodePattern>) {
        let node = located.node();
        let Some(predicate) = node.predicate() else {
            return;
        };
        let operator = predicate
            .operator()
            .expect("validated predicate has an operator");
        let mismatched = (operator.is_regex_op() && predicate.string_value().is_some())
            || (!operator.is_regex_op() && predicate.regex().is_some());
        if !mismatched {
            return;
        }

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
        let resolved = self.resolve_named_node_id(located).is_some();
        let mut builder = self.diag
            .report(DiagnosticKind::PredicateValueMismatch, located.span_of(predicate.syntax().text_range()))
            .detail(format!("predicate `{node_name} {predicate_text}` uses `{operator_text}`, which requires {required}, but supplies {supplied}"))
            .hint(format!("change the value in `{predicate_text}` to {required}, or choose an operator that accepts {supplied}"));
        if resolved {
            builder = builder.related_to(
                located.span_of(node_kind.text_range()),
                format!("predicate applies to `{node_name}`"),
            );
        }
        builder.emit();
    }

    /// Conservative entrypoint root admissibility: return `false` only when the
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
                let inner = cap.inner().expect("validated capture has a body");
                self.pattern_can_match_root(&located.wrap(inner), grammar_root, seen_refs)
            }
            Pattern::QuantifiedPattern(q) => {
                if q.is_optional() {
                    return true;
                }

                let inner = q.inner().expect("validated repeat has a body");
                self.pattern_can_match_root(&located.wrap(inner), grammar_root, seen_refs)
            }
            Pattern::Alternation(alternation) => {
                let mut saw_alternative = false;
                for alternative in alternation.alternatives() {
                    saw_alternative = true;
                    let body = alternative
                        .body()
                        .expect("validated alternative has a body");
                    if self.pattern_can_match_root(&located.wrap(body), grammar_root, seen_refs) {
                        return true;
                    }
                }
                !saw_alternative
            }
            Pattern::DefRef(def_ref) => {
                let def_id = self
                    .definitions
                    .reference_target(def_ref)
                    .expect("validated reference has a target");
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
            // Incoming fields are supplied by enclosing nodes. Standalone
            // eligibility handles this context requirement.
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

    /// Reject supertype patterns before lowering can emit a concrete-kind match that never fires.
    /// Bare `(expression)` gets a syntax-oriented diagnostic; marked `(expression#...)` gets the
    /// unsupported-feature diagnostic.
    /// Returns whether the node was rejected.
    fn reject_supertype_match(&mut self, located: &Located<NamedNodePattern>) -> bool {
        let node = located.node();
        let kind_token = node.kind_token().expect("validated node has a kind");
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
        let mut subtypes = self
            .grammar
            .collect_subtypes(id)
            .into_iter()
            .filter(|&sub| !self.grammar.is_supertype(sub))
            .map(|sub| {
                self.grammar
                    .node_kind(sub)
                    .expect("resolved subtype has a name")
            })
            .collect::<Vec<_>>();
        subtypes.sort_unstable();
        let mut builder = self.diag.report(kind, span).detail(name.as_str());
        if !subtypes.is_empty() {
            if subtypes.len() <= 8 {
                let alternatives = subtypes
                    .iter()
                    .map(|subtype| format!("({subtype})"))
                    .collect::<Vec<_>>()
                    .join(" ");
                builder = builder.hint(format!(
                    "match concrete subtypes with an alternation: `[{alternatives}]`"
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

struct SyntaxValidator<'a, 'b, 'q> {
    binder: &'a mut GrammarBinder<'b, 'q>,
}

impl Visitor for SyntaxValidator<'_, '_, '_> {
    fn visit_named_node_pattern(&mut self, node: &Located<ast::NamedNodePattern>) {
        if self.binder.reject_supertype_match(node) {
            return;
        }

        self.binder.validate_predicate(node);
        visitor::walk_named_node_pattern(self, node);
    }
}
