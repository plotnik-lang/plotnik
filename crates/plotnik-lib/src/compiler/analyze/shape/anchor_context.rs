//! External sibling context required by definition bodies.
//!
//! A boundary anchor in a reusable definition is not inherently invalid. Its
//! predecessor or follower may live at the call site, just as it would after
//! syntactically inlining the body. This analysis records whether successful
//! paths still require the caller's leading or trailing boundary after groups,
//! references, nullable paths, alternations, and quantifiers have composed.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use rowan::{TextRange, TextSize};

use crate::compiler::analyze::names::SymbolTable;
use crate::compiler::analyze::refs::DependencyAnalysis;
use crate::compiler::ids::DefId;
use crate::compiler::parse::ast::{Pattern, QuantifierKind, SeqItem};
use crate::core::Interner;

/// One path-sensitive external-context outcome.
///
/// `needs_left` and `needs_right` describe anchors that remain exposed at this
/// pattern's boundaries. A consuming neighbor supplied by sequence composition
/// discharges the corresponding need.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct ContextOutcome {
    consumed: bool,
    needs_left: bool,
    needs_right: bool,
}

impl ContextOutcome {
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
pub(super) struct ContextRelation {
    outcomes: BTreeSet<ContextOutcome>,
}

impl ContextRelation {
    fn impossible() -> Self {
        Self {
            outcomes: BTreeSet::new(),
        }
    }

    pub(super) fn singleton(outcome: ContextOutcome) -> Self {
        Self {
            outcomes: BTreeSet::from([outcome]),
        }
    }

    pub(super) fn identity() -> Self {
        Self::singleton(ContextOutcome::IDENTITY)
    }

    pub(super) fn atom() -> Self {
        Self::singleton(ContextOutcome::ATOM)
    }

    pub(super) fn anchor() -> Self {
        Self::singleton(ContextOutcome::ANCHOR)
    }

    fn union_with(&mut self, other: &Self) {
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

    fn then(&self, prefix: ContextOutcome, next: &Self, suffix: ContextOutcome) -> Self {
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
/// Ranges are grouped by `ContextOutcome`: composition only consults that
/// outcome's consumption flags, so merging ranges for equivalent outcomes
/// preserves path sensitivity without retaining every syntactic path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct AnchorRangeRelation {
    outcomes: BTreeMap<ContextOutcome, BoundaryAnchorRanges>,
}

impl AnchorRangeRelation {
    fn impossible() -> Self {
        Self {
            outcomes: BTreeMap::new(),
        }
    }

    fn singleton(outcome: ContextOutcome, ranges: BoundaryAnchorRanges) -> Self {
        Self {
            outcomes: BTreeMap::from([(outcome, ranges)]),
        }
    }

    fn identity() -> Self {
        Self::singleton(ContextOutcome::IDENTITY, BoundaryAnchorRanges::default())
    }

    pub(super) fn atom() -> Self {
        Self::singleton(ContextOutcome::ATOM, BoundaryAnchorRanges::default())
    }

    pub(super) fn anchor(range: TextRange) -> Self {
        Self::singleton(ContextOutcome::ANCHOR, BoundaryAnchorRanges::anchor(range))
    }

    fn from_context(relation: &ContextRelation) -> Self {
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

/// Definition-level fixed point for exported anchor context.
pub(crate) struct AnchorContextAnalysis<'a> {
    interner: &'a Interner,
    symbol_table: &'a SymbolTable,
    dependencies: &'a DependencyAnalysis,
    definitions: HashMap<DefId, ContextRelation>,
}

impl<'a> AnchorContextAnalysis<'a> {
    pub(crate) fn new(
        interner: &'a Interner,
        symbol_table: &'a SymbolTable,
        dependencies: &'a DependencyAnalysis,
    ) -> Self {
        let mut analysis = Self {
            interner,
            symbol_table,
            dependencies,
            definitions: HashMap::new(),
        };
        analysis.compute_definitions();
        analysis
    }

    pub(crate) fn definition_requires_external_context(&self, def_id: DefId) -> bool {
        self.definition(def_id).requires_external_context()
    }

    /// Anchor tokens authored in `def_id` whose boundary can remain exposed
    /// after transparent wrapper and nullable-path composition.
    pub(crate) fn exported_anchor_ranges(&self, def_id: DefId) -> Vec<TextRange> {
        let name = self
            .interner
            .resolve(self.dependencies.def_name_sym(def_id));
        let body = self
            .symbol_table
            .body(name)
            .expect("dependency analysis definitions have symbol-table bodies");
        self.compute_anchor_ranges(body).exported_ranges()
    }

    fn definition(&self, def_id: DefId) -> &ContextRelation {
        self.definitions
            .get(&def_id)
            .expect("every analyzed definition has an anchor-context relation")
    }

    fn compute_definitions(&mut self) {
        for scc in self.dependencies.sccs() {
            for &def_id in scc {
                self.definitions
                    .entry(def_id)
                    .or_insert_with(ContextRelation::impossible);
            }

            loop {
                let mut changed = false;
                for &def_id in scc {
                    let name = self
                        .interner
                        .resolve(self.dependencies.def_name_sym(def_id));
                    let body = self
                        .symbol_table
                        .body(name)
                        .expect("dependency analysis definitions have symbol-table bodies");
                    let relation = self.compute_pattern(body);
                    let current = self
                        .definitions
                        .get_mut(&def_id)
                        .expect("SCC definitions were initialized");
                    if *current != relation {
                        *current = relation;
                        changed = true;
                    }
                }
                if !changed {
                    break;
                }
            }
        }
    }

    fn compute_pattern(&self, pattern: &Pattern) -> ContextRelation {
        match pattern {
            // A named node supplies both boundary contexts to everything in its
            // child list, so none of its internal anchors escape the node.
            Pattern::NamedNodePattern(_)
            | Pattern::AnonymousNodePattern(_)
            | Pattern::NodeWildcard(_) => ContextRelation::atom(),
            Pattern::CapturedPattern(capture) => capture
                .inner()
                .map_or_else(ContextRelation::identity, |inner| {
                    self.compute_pattern(&inner)
                }),
            Pattern::FieldPattern(field) => field
                .value()
                .map_or_else(ContextRelation::identity, |value| {
                    self.compute_pattern(&value)
                }),
            Pattern::SeqPattern(sequence) => {
                let items: Vec<_> = sequence.items().collect();
                self.compute_items(&items)
            }
            Pattern::Alternation(alternation) => {
                let mut relation = ContextRelation::impossible();
                let mut had_alternative = false;
                for alternative in alternation.patterns() {
                    had_alternative = true;
                    relation.union_with(&self.compute_pattern(&alternative));
                }
                if had_alternative {
                    relation
                } else {
                    ContextRelation::identity()
                }
            }
            Pattern::QuantifiedPattern(quantified) => {
                let Some(inner) = quantified.inner() else {
                    return ContextRelation::identity();
                };
                let inner = self.compute_pattern(&inner);
                quantified
                    .quantifier_kind()
                    .map_or(inner.clone(), |kind| inner.quantified(kind))
            }
            Pattern::DefRef(reference) => {
                let Some(def_id) = reference.name().and_then(|name| {
                    self.dependencies
                        .def_id_for_name(self.interner, name.text())
                }) else {
                    // Name resolution owns the diagnostic. Treat its recovery
                    // node as one consumer so a neighboring anchor is not hidden.
                    return ContextRelation::atom();
                };
                self.definitions.get(&def_id).cloned().unwrap_or_else(|| {
                    panic!(
                        "anchor-context analysis resolved definition {def_id:?}, but its \
                             relation was not initialized before the reference was evaluated"
                    )
                })
            }
        }
    }

    fn compute_items(&self, items: &[SeqItem]) -> ContextRelation {
        let mut relation = ContextRelation::identity();
        for item in items {
            relation = relation.then(&match item {
                SeqItem::Anchor(_) => ContextRelation::anchor(),
                SeqItem::Pattern(pattern) => self.compute_pattern(pattern),
            });
        }
        relation
    }

    fn compute_anchor_ranges(&self, pattern: &Pattern) -> AnchorRangeRelation {
        match pattern {
            Pattern::NamedNodePattern(_)
            | Pattern::AnonymousNodePattern(_)
            | Pattern::NodeWildcard(_) => AnchorRangeRelation::atom(),
            Pattern::CapturedPattern(capture) => capture
                .inner()
                .map_or_else(AnchorRangeRelation::identity, |inner| {
                    self.compute_anchor_ranges(&inner)
                }),
            Pattern::FieldPattern(field) => field
                .value()
                .map_or_else(AnchorRangeRelation::identity, |value| {
                    self.compute_anchor_ranges(&value)
                }),
            Pattern::SeqPattern(sequence) => {
                let items: Vec<_> = sequence.items().collect();
                self.compute_anchor_range_items(&items)
            }
            Pattern::Alternation(alternation) => {
                let mut relation = AnchorRangeRelation::impossible();
                let mut had_alternative = false;
                for alternative in alternation.patterns() {
                    had_alternative = true;
                    relation.union_with(&self.compute_anchor_ranges(&alternative));
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
                let inner = self.compute_anchor_ranges(&inner);
                quantified
                    .quantifier_kind()
                    .map_or(inner.clone(), |kind| inner.quantified(kind))
            }
            Pattern::DefRef(reference) => {
                let Some(def_id) = reference.name().and_then(|name| {
                    self.dependencies
                        .def_id_for_name(self.interner, name.text())
                }) else {
                    return AnchorRangeRelation::atom();
                };
                AnchorRangeRelation::from_context(self.definition(def_id))
            }
        }
    }

    fn compute_anchor_range_items(&self, items: &[SeqItem]) -> AnchorRangeRelation {
        let mut relation = AnchorRangeRelation::identity();
        for item in items {
            relation = relation.then(&match item {
                SeqItem::Anchor(anchor) => AnchorRangeRelation::anchor(anchor.text_range()),
                SeqItem::Pattern(pattern) => self.compute_anchor_ranges(pattern),
            });
        }
        relation
    }
}
