//! Grammar-independent facts for definition bodies and authored patterns.
//!
//! Every fact in this module depends on the same definition graph and syntax
//! tree. They are therefore solved together, retained once, and borrowed by
//! validation, type inference, grammar checking, and lowering.

use std::collections::HashMap;

use rowan::TextRange;

use crate::compiler::analyze::boundary::{BoundaryRelation, FirstClass, PendingAnchor};
use crate::compiler::analyze::refs::DefinitionGraph;
use crate::compiler::ids::DefId;
use crate::compiler::parse::ast::{Pattern, QuantifierKind, SeqItem};
use crate::core::Interner;

use super::RootExtent;
use super::anchor_context::{self, AnchorContextRelation};

#[derive(Clone, PartialEq, Eq)]
struct PatternSummary {
    nullable: bool,
    root_extent: RootExtent,
    anchor_context: AnchorContextRelation,
    boundary: BoundaryRelation,
    may_match_anonymous_node: bool,
}

impl PatternSummary {
    fn bottom() -> Self {
        Self {
            nullable: false,
            root_extent: RootExtent::SingleNode,
            anchor_context: AnchorContextRelation::impossible(),
            boundary: BoundaryRelation::empty(),
            may_match_anonymous_node: false,
        }
    }

    fn atom(class: FirstClass) -> Self {
        Self {
            nullable: false,
            root_extent: RootExtent::SingleNode,
            anchor_context: AnchorContextRelation::atom(),
            boundary: BoundaryRelation::atom(class),
            may_match_anonymous_node: matches!(class, FirstClass::Anonymous | FirstClass::Either),
        }
    }

    fn recovery_identity() -> Self {
        Self {
            nullable: false,
            root_extent: RootExtent::SingleNode,
            anchor_context: AnchorContextRelation::identity(),
            boundary: BoundaryRelation::identity(),
            may_match_anonymous_node: false,
        }
    }

    fn sequence_identity() -> Self {
        Self {
            nullable: true,
            root_extent: RootExtent::SingleNode,
            anchor_context: AnchorContextRelation::identity(),
            boundary: BoundaryRelation::identity(),
            may_match_anonymous_node: false,
        }
    }

    fn undefined_reference() -> Self {
        Self {
            nullable: false,
            root_extent: RootExtent::SingleNode,
            anchor_context: AnchorContextRelation::atom(),
            boundary: BoundaryRelation::empty(),
            may_match_anonymous_node: false,
        }
    }

    fn into_definition_facts(self) -> DefinitionFacts {
        DefinitionFacts {
            nullable: self.nullable,
            root_extent: self.root_extent,
            anchor_context: self.anchor_context,
            boundary: self.boundary,
        }
    }

    fn into_authored_pattern_facts(self) -> AuthoredPatternFacts {
        AuthoredPatternFacts {
            nullable: self.nullable,
            boundary: self.boundary,
            may_match_anonymous_node: self.may_match_anonymous_node,
        }
    }
}

struct DefinitionFacts {
    nullable: bool,
    root_extent: RootExtent,
    anchor_context: AnchorContextRelation,
    boundary: BoundaryRelation,
}

struct AuthoredPatternFacts {
    nullable: bool,
    boundary: BoundaryRelation,
    may_match_anonymous_node: bool,
}

/// Frozen classification of every admitted definition and authored pattern.
pub(crate) struct PatternFacts {
    definitions: Vec<DefinitionFacts>,
    patterns: HashMap<Pattern, AuthoredPatternFacts>,
}

impl PatternFacts {
    pub(crate) fn analyze(interner: &Interner, definitions: &DefinitionGraph) -> Self {
        let mut definition_summaries = definitions
            .ids_in_def_id_order()
            .map(|_| PatternSummary::bottom())
            .collect::<Vec<_>>();

        // Every component is a finite monotone lattice. Solving their product
        // gives the same least fixed points as separate passes while resolving
        // each reference and walking each definition body only once per round.
        for scc in definitions.sccs() {
            loop {
                let mut changed = false;
                for &def_id in scc {
                    let body = definitions.definition(def_id).body();
                    let next =
                        summarize_pattern(body, &definition_summaries, definitions, interner, None);
                    let current = definition_summary_mut(&mut definition_summaries, def_id);
                    if *current != next {
                        *current = next;
                        changed = true;
                    }
                }
                if !changed {
                    break;
                }
            }
        }

        // The fixed point answers references. Materialize each authored
        // subpattern once so downstream phases borrow projections rather than
        // recursively reinterpreting the same syntax.
        let mut pattern_summaries = HashMap::new();
        for def_id in definitions.ids_in_def_id_order() {
            retain_pattern_summaries(
                definitions.definition(def_id).body(),
                &definition_summaries,
                definitions,
                interner,
                &mut pattern_summaries,
            );
        }

        let definitions = definition_summaries
            .into_iter()
            .map(PatternSummary::into_definition_facts)
            .collect();
        let patterns = pattern_summaries
            .into_iter()
            .map(|(pattern, summary)| (pattern, summary.into_authored_pattern_facts()))
            .collect();

        Self {
            definitions,
            patterns,
        }
    }

    pub(crate) fn definition_is_nullable(&self, def_id: DefId) -> bool {
        self.definition(def_id).nullable
    }

    pub(crate) fn definition_root_extent(&self, def_id: DefId) -> RootExtent {
        self.definition(def_id).root_extent
    }

    pub(crate) fn is_entry_point_eligible(&self, def_id: DefId) -> bool {
        let facts = self.definition(def_id);
        facts.root_extent == RootExtent::SingleNode
            && !facts.anchor_context.requires_external_context()
    }

    pub(crate) fn definition_requires_external_anchor_context(&self, def_id: DefId) -> bool {
        self.definition(def_id)
            .anchor_context
            .requires_external_context()
    }

    pub(crate) fn exported_anchor_ranges(
        &self,
        def_id: DefId,
        definitions: &DefinitionGraph,
        interner: &Interner,
    ) -> Vec<TextRange> {
        anchor_context::exported_anchor_ranges(self, def_id, definitions, interner)
    }

    pub(crate) fn pattern_is_nullable(&self, pattern: &Pattern) -> bool {
        self.pattern(pattern).nullable
    }

    pub(crate) fn pattern_may_match_anonymous_node(&self, pattern: &Pattern) -> bool {
        self.pattern(pattern).may_match_anonymous_node
    }

    pub(crate) fn pattern_boundary_relation(&self, pattern: &Pattern) -> &BoundaryRelation {
        &self.pattern(pattern).boundary
    }

    pub(crate) fn items_boundary_relation(&self, items: &[SeqItem]) -> BoundaryRelation {
        let mut relation = BoundaryRelation::identity();
        for item in items {
            relation = match item {
                SeqItem::Anchor(anchor) => relation.anchor(if anchor.is_exact() {
                    PendingAnchor::Exact
                } else {
                    PendingAnchor::Soft
                }),
                SeqItem::Pattern(pattern) => relation.then(self.pattern_boundary_relation(pattern)),
            };
        }
        relation
    }

    pub(crate) fn definition_boundary_relation(&self, def_id: DefId) -> &BoundaryRelation {
        &self.definition(def_id).boundary
    }

    pub(super) fn definition_anchor_context(&self, def_id: DefId) -> &AnchorContextRelation {
        &self.definition(def_id).anchor_context
    }

    fn definition(&self, def_id: DefId) -> &DefinitionFacts {
        self.definitions
            .get(def_id.index())
            .expect("pattern-fact lookup must use an admitted DefId")
    }

    fn pattern(&self, pattern: &Pattern) -> &AuthoredPatternFacts {
        self.patterns
            .get(pattern)
            .expect("pattern-fact lookup must use an analyzed pattern")
    }
}

fn definition_summary(summaries: &[PatternSummary], def_id: DefId) -> &PatternSummary {
    summaries
        .get(def_id.index())
        .expect("pattern summary lookup must use an admitted DefId")
}

fn definition_summary_mut(summaries: &mut [PatternSummary], def_id: DefId) -> &mut PatternSummary {
    summaries
        .get_mut(def_id.index())
        .expect("pattern summary lookup must use an admitted DefId")
}

fn retain_pattern_summaries(
    pattern: &Pattern,
    definition_summaries: &[PatternSummary],
    definitions: &DefinitionGraph,
    interner: &Interner,
    patterns: &mut HashMap<Pattern, PatternSummary>,
) {
    if patterns.contains_key(pattern) {
        return;
    }
    for child in pattern.children() {
        retain_pattern_summaries(
            &child,
            definition_summaries,
            definitions,
            interner,
            patterns,
        );
    }
    let summary = summarize_pattern(
        pattern,
        definition_summaries,
        definitions,
        interner,
        Some(patterns),
    );
    patterns.insert(pattern.clone(), summary);
}

fn summarize_child(
    pattern: &Pattern,
    definition_summaries: &[PatternSummary],
    definitions: &DefinitionGraph,
    interner: &Interner,
    patterns: Option<&HashMap<Pattern, PatternSummary>>,
) -> PatternSummary {
    if let Some(summary) = patterns.and_then(|patterns| patterns.get(pattern)) {
        return summary.clone();
    }
    summarize_pattern(
        pattern,
        definition_summaries,
        definitions,
        interner,
        patterns,
    )
}

fn summarize_pattern(
    pattern: &Pattern,
    definition_summaries: &[PatternSummary],
    definitions: &DefinitionGraph,
    interner: &Interner,
    patterns: Option<&HashMap<Pattern, PatternSummary>>,
) -> PatternSummary {
    match pattern {
        Pattern::NamedNodePattern(_) => PatternSummary::atom(FirstClass::Named),
        Pattern::AnonymousNodePattern(_) => PatternSummary::atom(FirstClass::Anonymous),
        Pattern::NodeWildcard(_) => PatternSummary::atom(FirstClass::Either),
        Pattern::CapturedPattern(capture) => {
            capture
                .inner()
                .map_or_else(PatternSummary::recovery_identity, |inner| {
                    summarize_child(
                        &inner,
                        definition_summaries,
                        definitions,
                        interner,
                        patterns,
                    )
                })
        }
        Pattern::FieldPattern(field) => {
            let Some(value) = field.value() else {
                return PatternSummary::recovery_identity();
            };
            let mut summary = summarize_child(
                &value,
                definition_summaries,
                definitions,
                interner,
                patterns,
            );
            // Field values are validated as exactly one node. Recovery keeps
            // their definition-level extent and nullability conservative.
            summary.nullable = false;
            summary.root_extent = RootExtent::SingleNode;
            summary
        }
        Pattern::SeqPattern(sequence) => {
            let mut summary = PatternSummary::sequence_identity();
            let mut pattern_count = 0;
            for item in sequence.items() {
                match item {
                    SeqItem::Anchor(anchor) => {
                        summary.anchor_context = summary
                            .anchor_context
                            .then(&AnchorContextRelation::anchor());
                        summary.boundary = summary.boundary.anchor(if anchor.is_exact() {
                            PendingAnchor::Exact
                        } else {
                            PendingAnchor::Soft
                        });
                    }
                    SeqItem::Pattern(pattern) => {
                        let child = summarize_child(
                            &pattern,
                            definition_summaries,
                            definitions,
                            interner,
                            patterns,
                        );
                        summary.nullable &= child.nullable;
                        summary.root_extent = match pattern_count {
                            0 => child.root_extent,
                            _ => RootExtent::NotSingleNode,
                        };
                        summary.anchor_context = summary.anchor_context.then(&child.anchor_context);
                        summary.boundary = summary.boundary.then(&child.boundary);
                        summary.may_match_anonymous_node |= child.may_match_anonymous_node;
                        pattern_count += 1;
                    }
                }
            }
            summary
        }
        Pattern::Alternation(alternation) => {
            let mut summary = PatternSummary::bottom();
            let mut had_alternative = false;
            for body in alternation.patterns() {
                had_alternative = true;
                let child =
                    summarize_child(&body, definition_summaries, definitions, interner, patterns);
                summary.nullable |= child.nullable;
                summary.root_extent = summary.root_extent.combine(child.root_extent);
                summary.anchor_context.union_with(&child.anchor_context);
                summary.boundary.union_with(&child.boundary);
                summary.may_match_anonymous_node |= child.may_match_anonymous_node;
            }
            if !had_alternative {
                summary.anchor_context = AnchorContextRelation::identity();
                summary.boundary = BoundaryRelation::identity();
            }
            summary
        }
        Pattern::QuantifiedPattern(quantified) => {
            let Some(inner) = quantified.inner() else {
                return PatternSummary::recovery_identity();
            };
            let mut summary = summarize_child(
                &inner,
                definition_summaries,
                definitions,
                interner,
                patterns,
            );
            summary.root_extent = RootExtent::NotSingleNode;
            if let Some(kind) = quantified.quantifier_kind() {
                summary.nullable =
                    matches!(kind, QuantifierKind::Optional | QuantifierKind::ZeroOrMore);
                summary.anchor_context = summary.anchor_context.quantified(kind);
                summary.boundary = summary.boundary.quantified(kind);
            }
            summary
        }
        Pattern::DefRef(reference) => {
            let Some(def_id) = reference
                .name()
                .and_then(|name| definitions.id_for_name(interner, name.text()))
            else {
                return PatternSummary::undefined_reference();
            };
            definition_summary(definition_summaries, def_id).clone()
        }
    }
}
