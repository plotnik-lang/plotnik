//! Matching structure resolved once at the grammar boundary. Source nodes remain
//! attached for diagnostics, but automaton construction uses only these IDs and gaps.

use std::collections::HashMap;

use indexmap::IndexMap;
use rowan::TextRange;

use crate::compiler::analyze::Located;
use crate::compiler::analyze::anchors::{AnchorSemantics, GapClass, has_direct_alternative_nav};
use crate::compiler::analyze::refs::DefinitionGraph;
use crate::compiler::analyze::shape::PatternFacts;
use crate::compiler::diagnostics::Span;
use crate::compiler::diagnostics::source::{SourceId, SourceMap};
use crate::compiler::ids::DefId;
use crate::compiler::parse::ast::{self, QuantifierKind, SeqItem, token_src};
use crate::compiler::parse::cst::SyntaxKind;
use crate::core::grammar::Grammar;
use crate::core::{NodeFieldId, NodeKind, NodeKindId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct PatternId(usize);

impl PatternId {
    pub(super) fn index(self) -> usize {
        self.0
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum KindConstraint {
    Exact(NodeKindId),
    AnyNamed,
    AnyNode,
    /// ERROR and MISSING are outside the retained grammar's tree model.
    Unconstrained,
}

pub(super) struct Pattern {
    pub(super) origin: Located<ast::Pattern>,
    pub(super) kind: PatternKind,
}

impl Pattern {
    /// Captures are absent from matching, but remain in provenance for full spans.
    pub(super) fn syntax(&self) -> Located<ast::Pattern> {
        self.origin.wrap(uncaptured(self.origin.node().clone()))
    }
}

pub(super) enum PatternKind {
    Node(Node),
    Leaf(KindConstraint),
    Field(NodeFieldId, PatternId),
    Sequence(Vec<Item>),
    Alternative(Vec<PatternId>),
    Repeat(QuantifierKind, PatternId),
    Reference(DefId),
}

pub(super) struct Node {
    pub(super) kind: KindConstraint,
    pub(super) violation: Option<NodeViolation>,
    pub(super) children: Vec<Item>,
    pub(super) trailing: GapClass,
    pub(super) trailing_anchor: Option<Span>,
    pub(super) negated_fields: Vec<NegatedField>,
}

#[derive(Clone, Copy)]
pub(super) struct NegatedField {
    pub(super) id: NodeFieldId,
    pub(super) span: Span,
}

impl Node {
    pub(super) fn constrains_children(&self) -> bool {
        !self.children.is_empty()
            || self.trailing != GapClass::Any
            || !self.negated_fields.is_empty()
    }
}

#[derive(Clone, Copy)]
pub(super) enum NodeViolation {
    PredicateOnNonLeaf,
    FieldNotOnKind(NegatedField),
}

pub(super) struct Item {
    pub(super) pattern: PatternId,
    pub(super) gap: GapClass,
    pub(super) anchor: Option<Span>,
}

#[derive(Default)]
pub(super) struct Model {
    patterns: Vec<Pattern>,
    definitions: IndexMap<DefId, PatternId>,
}

impl Model {
    pub(super) fn resolve(
        grammar: &Grammar,
        definitions: &DefinitionGraph,
        facts: &PatternFacts,
        sources: &SourceMap,
        kinds: &HashMap<NodeKind<&str>, Option<NodeKindId>>,
        fields: &HashMap<&str, Option<NodeFieldId>>,
    ) -> Self {
        let mut builder = Builder {
            model: Self::default(),
            grammar,
            definitions,
            anchors: AnchorSemantics::new(facts),
            sources,
            kinds,
            fields,
        };
        for &definition in definitions.ids_in_declaration_order() {
            let body = definitions.definition(definition).located_body();
            let pattern = builder.pattern(body);
            builder.model.definitions.insert(definition, pattern);
        }
        builder.model
    }

    pub(super) fn pattern(&self, id: PatternId) -> &Pattern {
        &self.patterns[id.index()]
    }

    pub(super) fn definition(&self, id: DefId) -> PatternId {
        self.definitions[&id]
    }

    pub(super) fn definitions(&self) -> impl Iterator<Item = PatternId> + '_ {
        self.definitions.values().copied()
    }

    pub(super) fn len(&self) -> usize {
        self.patterns.len()
    }

    pub(super) fn node(&self, id: PatternId) -> &Node {
        let PatternKind::Node(node) = &self.pattern(id).kind else {
            panic!("a node goal must refer to a resolved node pattern");
        };
        node
    }

    pub(super) fn node_origin(&self, id: PatternId) -> Located<ast::NamedNodePattern> {
        let origin = self.pattern(id).syntax();
        let ast::Pattern::NamedNodePattern(node) = origin.node() else {
            panic!("a node goal must originate at a named node pattern");
        };
        origin.wrap(node.clone())
    }
}

struct Builder<'a> {
    model: Model,
    grammar: &'a Grammar,
    definitions: &'a DefinitionGraph,
    anchors: AnchorSemantics<'a>,
    sources: &'a SourceMap,
    kinds: &'a HashMap<NodeKind<&'a str>, Option<NodeKindId>>,
    fields: &'a HashMap<&'a str, Option<NodeFieldId>>,
}

impl Builder<'_> {
    fn pattern(&mut self, origin: Located<ast::Pattern>) -> PatternId {
        let source = origin.source();
        let syntax = uncaptured(origin.node().clone());
        let kind = match &syntax {
            ast::Pattern::NamedNodePattern(node) => {
                let kind = if node.is_any() {
                    KindConstraint::AnyNamed
                } else {
                    let token = node.kind_token().expect("validated node has a kind");
                    if matches!(token.kind(), SyntaxKind::KwError | SyntaxKind::KwMissing) {
                        KindConstraint::Unconstrained
                    } else {
                        self.kind(NodeKind::Named(token_src(
                            &token,
                            self.sources.content(source),
                        )))
                    }
                };
                let items: Vec<_> = node.items().collect();
                let (has_trailing, nav) = self.anchors.check_trailing_anchor(&items);
                let trailing = if has_trailing {
                    GapClass::from_nav(nav.expect("trailing anchor has a navigation"))
                        .expect("trailing anchor uses sibling navigation")
                } else {
                    GapClass::Any
                };
                let negated_fields: Vec<_> = node
                    .syntax()
                    .children()
                    .filter_map(ast::NegatedField::cast)
                    .map(|field| {
                        let token = field.name().expect("validated field has a name");
                        NegatedField {
                            id: self.field(token.text()),
                            span: Span::new(source, token.text_range()),
                        }
                    })
                    .collect();
                // Kind-wide restrictions cannot vary with a production or caller.
                // Keep the reason so a dead branch can later be explained, without
                // issuing an error when another branch can match.
                let violation = match kind {
                    KindConstraint::Exact(kind) => {
                        if node.predicate().is_some()
                            && self.grammar.has_declared_child_structure(kind)
                        {
                            Some(NodeViolation::PredicateOnNonLeaf)
                        } else {
                            negated_fields.iter().find_map(|&field| {
                                (!self.grammar.has_field(kind, field.id))
                                    .then_some(NodeViolation::FieldNotOnKind(field))
                            })
                        }
                    }
                    _ => None,
                };
                PatternKind::Node(Node {
                    kind,
                    violation,
                    children: self.items(source, &items, true),
                    trailing,
                    trailing_anchor: trailing_anchor(&items).map(|range| Span::new(source, range)),
                    negated_fields,
                })
            }
            ast::Pattern::AnonymousNodePattern(node) => {
                let token = node.value().expect("validated anonymous node has a value");
                PatternKind::Leaf(self.kind(NodeKind::Anonymous(token_src(
                    &token,
                    self.sources.content(source),
                ))))
            }
            ast::Pattern::NodeWildcard(_) => PatternKind::Leaf(KindConstraint::AnyNode),
            ast::Pattern::CapturedPattern(_) => unreachable!("captures were unwrapped"),
            ast::Pattern::FieldPattern(field) => {
                let id = self.field(field.name().expect("validated field has a name").text());
                let body =
                    self.pattern(origin.wrap(field.value().expect("validated field has a value")));
                PatternKind::Field(id, body)
            }
            ast::Pattern::SeqPattern(sequence) => PatternKind::Sequence(self.items(
                source,
                &sequence.items().collect::<Vec<_>>(),
                false,
            )),
            ast::Pattern::Alternation(_) => PatternKind::Alternative(
                syntax
                    .children()
                    .map(|child| self.pattern(origin.wrap(child)))
                    .collect(),
            ),
            ast::Pattern::QuantifiedPattern(repeat) => PatternKind::Repeat(
                repeat
                    .quantifier_kind()
                    .expect("validated repeat has an operator"),
                self.pattern(origin.wrap(repeat.inner().expect("validated repeat has a body"))),
            ),
            ast::Pattern::DefRef(reference) => PatternKind::Reference(
                self.definitions
                    .reference_target(reference)
                    .expect("validated reference has a target"),
            ),
        };
        let id = PatternId(self.model.patterns.len());
        self.model.patterns.push(Pattern { origin, kind });
        id
    }

    fn items(&mut self, source: SourceId, items: &[SeqItem], inside_node: bool) -> Vec<Item> {
        self.anchors
            .compute_nav_modes(items, inside_node)
            .into_iter()
            .map(|(index, nav)| {
                let SeqItem::Pattern(pattern) = &items[index] else {
                    unreachable!("anchor navigation is indexed by pattern items");
                };
                let mut gap = nav.map_or(GapClass::Any, |nav| {
                    GapClass::from_nav(nav).expect("sequence entry uses sibling navigation")
                });
                // Direct alternatives choose their own navigation in the matcher. Keep
                // their conservative shared gap, without weakening exact anchors.
                if gap == GapClass::ExtrasOnly && has_direct_alternative_nav(pattern) {
                    gap = GapClass::AnonymousAndExtras;
                }
                Item {
                    pattern: self.pattern(Located::new(source, pattern.clone())),
                    gap,
                    anchor: anchor_range(items[..index].iter().rev())
                        .map(|range| Span::new(source, range)),
                }
            })
            .collect()
    }

    fn kind(&self, name: NodeKind<&str>) -> KindConstraint {
        KindConstraint::Exact(self.kinds[&name].expect("validated kind was resolved by binding"))
    }

    fn field(&self, name: &str) -> NodeFieldId {
        self.fields[name].expect("validated field was resolved by binding")
    }
}

// Captures neither add matching operations nor consume the automaton builder's
// recursion budget. Strip them iteratively to avoid recursion here as well.
fn uncaptured(mut pattern: ast::Pattern) -> ast::Pattern {
    while let ast::Pattern::CapturedPattern(capture) = pattern {
        pattern = capture.inner().expect("validated capture has a body");
    }
    pattern
}

// Source provenance follows the same item boundaries used during resolution.
// It does not compute navigation or decide what an anchor permits.
fn anchor_range<'a>(items: impl Iterator<Item = &'a SeqItem>) -> Option<TextRange> {
    items
        .map_while(|item| match item {
            SeqItem::Anchor(anchor) => Some(anchor.syntax().text_range()),
            SeqItem::Pattern(_) => None,
        })
        .reduce(TextRange::cover)
}

fn trailing_anchor(items: &[SeqItem]) -> Option<TextRange> {
    if let [SeqItem::Pattern(ast::Pattern::SeqPattern(sequence))] = items {
        return trailing_anchor(&sequence.items().collect::<Vec<_>>());
    }
    anchor_range(items.iter().rev())
}
