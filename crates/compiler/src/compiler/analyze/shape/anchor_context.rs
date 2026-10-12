//! External sibling context required by definition bodies.
//!
//! A boundary anchor in a reusable definition is not inherently invalid. Its
//! predecessor or follower may live at the call site, just as it would after
//! syntactically inlining the body. This analysis records whether successful
//! paths still require the caller's leading or trailing boundary after groups,
//! references, nullable paths, alternations, and quantifiers have composed.

use std::collections::{BTreeMap, BTreeSet};

use rowan::{TextRange, TextSize};

use crate::compiler::analyze::refs::DefinitionGraph;
use crate::compiler::ids::DefId;
use crate::compiler::parse::ast::{Pattern, QuantifierKind, SeqItem};

use super::PatternFacts;

/// One path-sensitive external-context outcome.
///
/// `needs_left` and `needs_right` describe anchors that remain exposed at this
/// pattern's boundaries. A consuming neighbor supplied by sequence composition
/// discharges the corresponding need.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct AnchorContextOutcome {
    consumed: bool,
    needs_left: bool,
    needs_right: bool,
}

impl AnchorContextOutcome {
    pub(super) const IDENTITY: Self = Self {
        consumed: false,
        needs_left: false,
        needs_right: false,
    };

    pub(super) const ATOM: Self = Self {
        consumed: true,
        needs_left: false,
        needs_right: false,
    };

    pub(super) const ANCHOR: Self = Self {
        consumed: false,
        needs_left: true,
        needs_right: true,
    };

    pub(super) fn then(self, next: Self) -> Self {
        Self {
            consumed: self.consumed || next.consumed,
            needs_left: self.needs_left || (!self.consumed && next.needs_left),
            needs_right: next.needs_right || (!next.consumed && self.needs_right),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct AnchorContextRelation {
    outcomes: BTreeSet<AnchorContextOutcome>,
}

impl AnchorContextRelation {
    pub(super) fn impossible() -> Self {
        Self {
            outcomes: BTreeSet::new(),
        }
    }

    pub(super) fn singleton(outcome: AnchorContextOutcome) -> Self {
        Self {
            outcomes: BTreeSet::from([outcome]),
        }
    }

    pub(super) fn identity() -> Self {
        Self::singleton(AnchorContextOutcome::IDENTITY)
    }

    pub(super) fn atom() -> Self {
        Self::singleton(AnchorContextOutcome::ATOM)
    }

    pub(super) fn anchor() -> Self {
        Self::singleton(AnchorContextOutcome::ANCHOR)
    }

    pub(super) fn union_with(&mut self, other: &Self) {
        self.outcomes.extend(other.outcomes.iter().copied());
    }

    pub(super) fn then(&self, next: &Self) -> Self {
        let mut composed = Self::impossible();
        for prefix in &self.outcomes {
            for suffix in &next.outcomes {
                composed.outcomes.insert(prefix.then(*suffix));
            }
        }
        composed
    }

    fn consuming_only(&self) -> Self {
        Self {
            outcomes: self
                .outcomes
                .iter()
                .copied()
                .filter(|outcome| outcome.consumed)
                .collect(),
        }
    }

    pub(super) fn quantified(&self, kind: QuantifierKind) -> Self {
        match kind {
            QuantifierKind::Optional => {
                let mut optional = Self::identity();
                optional.union_with(&self.consuming_only());
                optional
            }
            QuantifierKind::ZeroOrMore => self.consuming_closure(true),
            QuantifierKind::OneOrMore => self.consuming_closure(false),
        }
    }

    /// Repetition admits only consuming inner outcomes. Empty iterations do
    /// not execute, so their anchors cannot create an external obligation.
    fn consuming_closure(&self, include_identity: bool) -> Self {
        let iterations = self.consuming_only();
        let mut closure = if include_identity {
            Self::identity()
        } else {
            Self::impossible()
        };
        let mut pending: Vec<_> = iterations.outcomes.iter().copied().collect();
        let mut seen = BTreeSet::new();

        while let Some(outcome) = pending.pop() {
            if !seen.insert(outcome) {
                continue;
            }
            closure.outcomes.insert(outcome);
            pending.extend(iterations.outcomes.iter().map(|next| outcome.then(*next)));
        }

        closure
    }

    pub(super) fn requires_external_context(&self) -> bool {
        self.outcomes
            .iter()
            .any(|outcome| outcome.needs_left || outcome.needs_right)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct BoundaryAnchorRanges {
    left: BTreeSet<(TextSize, TextSize)>,
    right: BTreeSet<(TextSize, TextSize)>,
}

impl BoundaryAnchorRanges {
    fn anchor(range: TextRange) -> Self {
        let bounds = (range.start(), range.end());
        Self {
            left: BTreeSet::from([bounds]),
            right: BTreeSet::from([bounds]),
        }
    }

    fn then(
        &self,
        prefix: AnchorContextOutcome,
        next: &Self,
        suffix: AnchorContextOutcome,
    ) -> Self {
        let mut combined = Self::default();
        combined.left.extend(self.left.iter().copied());
        if !prefix.consumed {
            combined.left.extend(next.left.iter().copied());
        }

        combined.right.extend(next.right.iter().copied());
        if !suffix.consumed {
            combined.right.extend(self.right.iter().copied());
        }
        combined
    }

    fn union_with(&mut self, other: &Self) {
        self.left.extend(other.left.iter().copied());
        self.right.extend(other.right.iter().copied());
    }
}

/// Context outcomes annotated with authored anchors that remain exposed.
///
/// Ranges are grouped by `AnchorContextOutcome`: composition only consults that
/// outcome's consumption flags, so merging ranges for equivalent outcomes
/// preserves path sensitivity without retaining every syntactic path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct AnchorRangeRelation {
    outcomes: BTreeMap<AnchorContextOutcome, BoundaryAnchorRanges>,
}

impl AnchorRangeRelation {
    fn impossible() -> Self {
        Self {
            outcomes: BTreeMap::new(),
        }
    }

    fn singleton(outcome: AnchorContextOutcome, ranges: BoundaryAnchorRanges) -> Self {
        Self {
            outcomes: BTreeMap::from([(outcome, ranges)]),
        }
    }

    fn identity() -> Self {
        Self::singleton(
            AnchorContextOutcome::IDENTITY,
            BoundaryAnchorRanges::default(),
        )
    }

    pub(super) fn atom() -> Self {
        Self::singleton(AnchorContextOutcome::ATOM, BoundaryAnchorRanges::default())
    }

    pub(super) fn anchor(range: TextRange) -> Self {
        Self::singleton(
            AnchorContextOutcome::ANCHOR,
            BoundaryAnchorRanges::anchor(range),
        )
    }

    fn from_anchor_context(relation: &AnchorContextRelation) -> Self {
        Self {
            outcomes: relation
                .outcomes
                .iter()
                .copied()
                .map(|outcome| (outcome, BoundaryAnchorRanges::default()))
                .collect(),
        }
    }

    pub(super) fn union_with(&mut self, other: &Self) {
        for (&outcome, ranges) in &other.outcomes {
            self.outcomes.entry(outcome).or_default().union_with(ranges);
        }
    }

    pub(super) fn then(&self, next: &Self) -> Self {
        let mut composed = Self::impossible();
        for (&prefix, prefix_ranges) in &self.outcomes {
            for (&suffix, suffix_ranges) in &next.outcomes {
                let outcome = prefix.then(suffix);
                let ranges = prefix_ranges.then(prefix, suffix_ranges, suffix);
                composed
                    .outcomes
                    .entry(outcome)
                    .or_default()
                    .union_with(&ranges);
            }
        }
        composed
    }

    fn consuming_only(&self) -> Self {
        Self {
            outcomes: self
                .outcomes
                .iter()
                .filter(|(outcome, _)| outcome.consumed)
                .map(|(&outcome, ranges)| (outcome, ranges.clone()))
                .collect(),
        }
    }

    pub(super) fn quantified(&self, kind: QuantifierKind) -> Self {
        match kind {
            QuantifierKind::Optional => {
                let mut optional = Self::identity();
                optional.union_with(&self.consuming_only());
                optional
            }
            QuantifierKind::ZeroOrMore => self.consuming_closure(true),
            QuantifierKind::OneOrMore => self.consuming_closure(false),
        }
    }

    fn consuming_closure(&self, include_identity: bool) -> Self {
        let iterations = self.consuming_only();
        let mut closure = iterations.clone();
        if include_identity {
            closure.union_with(&Self::identity());
        }

        loop {
            let repeated = closure.then(&iterations);
            let mut expanded = closure.clone();
            expanded.union_with(&repeated);
            if expanded == closure {
                return closure;
            }
            closure = expanded;
        }
    }

    pub(super) fn exported_ranges(&self) -> Vec<TextRange> {
        let mut ranges = BTreeSet::new();
        for outcome in self.outcomes.values() {
            ranges.extend(outcome.left.iter().copied());
            ranges.extend(outcome.right.iter().copied());
        }
        ranges
            .into_iter()
            .map(|(start, end)| TextRange::new(start, end))
            .collect()
    }
}

/// Anchor tokens authored in `def_id` whose boundary can remain exposed after
/// transparent wrapper and nullable-path composition.
pub(super) fn exported_anchor_ranges(
    pattern_facts: &PatternFacts,
    def_id: DefId,
    definitions: &DefinitionGraph,
) -> Vec<TextRange> {
    let body = definitions.definition(def_id).body();
    compute_anchor_ranges(body, pattern_facts, definitions).exported_ranges()
}

fn compute_anchor_ranges(
    pattern: &Pattern,
    pattern_facts: &PatternFacts,
    definitions: &DefinitionGraph,
) -> AnchorRangeRelation {
    match pattern {
        Pattern::NamedNodePattern(_)
        | Pattern::AnonymousNodePattern(_)
        | Pattern::NodeWildcard(_) => AnchorRangeRelation::atom(),
        Pattern::CapturedPattern(capture) => capture
            .inner()
            .map_or_else(AnchorRangeRelation::identity, |inner| {
                compute_anchor_ranges(&inner, pattern_facts, definitions)
            }),
        Pattern::FieldPattern(field) => field
            .value()
            .map_or_else(AnchorRangeRelation::identity, |value| {
                compute_anchor_ranges(&value, pattern_facts, definitions)
            }),
        Pattern::SeqPattern(sequence) => {
            let mut relation = AnchorRangeRelation::identity();
            for item in sequence.items() {
                relation = relation.then(&match item {
                    SeqItem::Anchor(anchor) => AnchorRangeRelation::anchor(anchor.text_range()),
                    SeqItem::Pattern(pattern) => {
                        compute_anchor_ranges(&pattern, pattern_facts, definitions)
                    }
                });
            }
            relation
        }
        Pattern::Alternation(alternation) => {
            let mut relation = AnchorRangeRelation::impossible();
            let mut had_alternative = false;
            for alternative in alternation.patterns() {
                had_alternative = true;
                relation.union_with(&compute_anchor_ranges(
                    &alternative,
                    pattern_facts,
                    definitions,
                ));
            }
            if had_alternative {
                relation
            } else {
                AnchorRangeRelation::identity()
            }
        }
        Pattern::QuantifiedPattern(quantified) => {
            let Some(inner) = quantified.inner() else {
                return AnchorRangeRelation::identity();
            };
            let inner = compute_anchor_ranges(&inner, pattern_facts, definitions);
            quantified
                .quantifier_kind()
                .map_or(inner.clone(), |kind| inner.quantified(kind))
        }
        Pattern::DefRef(reference) => {
            let Some(def_id) = definitions.reference_target(reference) else {
                return AnchorRangeRelation::atom();
            };
            AnchorRangeRelation::from_anchor_context(
                pattern_facts.definition_anchor_context(def_id),
            )
        }
    }
}
