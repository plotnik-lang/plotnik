//! Path-sensitive sibling-boundary semantics.
//!
//! Lowering needs more than nullability. A structural pattern may consume a
//! named node, consume a node that must be treated conservatively by a future
//! soft anchor, or consume nothing while leaving an anchor pending. This module
//! computes that finite relation independently of bytecode and runtime ABI
//! choices.

use std::collections::{BTreeSet, HashMap};

use crate::compiler::analyze::names::SymbolTable;
use crate::compiler::analyze::refs::DependencyAnalysis;
use crate::compiler::ids::DefId;
use crate::compiler::parse::ast::{Pattern, QuantifierKind, SeqItem};
use crate::core::Interner;

const FIRST_CLASS_COUNT: usize = 4;
const PENDING_ANCHOR_COUNT: usize = 3;
const STATE_COUNT: usize = FIRST_CLASS_COUNT * PENDING_ANCHOR_COUNT;

/// How the most recent consumer must be classified for future soft anchors.
///
/// `Either` is intentionally distinct from a union of `Named` and `Anonymous`:
/// one wildcard match has no control-flow branch that can select two different
/// continuations. It therefore remains one descriptive outcome until the
/// operational quotient conservatively merges it with `Anonymous`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum FirstClass {
    Empty,
    Named,
    Anonymous,
    Either,
}

impl FirstClass {
    pub(crate) const ALL: [Self; FIRST_CLASS_COUNT] =
        [Self::Empty, Self::Named, Self::Anonymous, Self::Either];

    fn index(self) -> usize {
        match self {
            Self::Empty => 0,
            Self::Named => 1,
            Self::Anonymous => 2,
            Self::Either => 3,
        }
    }
}

/// Anchor constraint waiting for the next consumer or the enclosing boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum PendingAnchor {
    None,
    Soft,
    Exact,
}

impl PendingAnchor {
    pub(crate) const ALL: [Self; PENDING_ANCHOR_COUNT] = [Self::None, Self::Soft, Self::Exact];

    fn index(self) -> usize {
        match self {
            Self::None => 0,
            Self::Soft => 1,
            Self::Exact => 2,
        }
    }

    /// Consecutive anchors intersect their permissions. Exact is absorbing:
    /// a later soft anchor can never loosen it.
    pub(crate) fn tighten(self, other: Self) -> Self {
        std::cmp::max(self, other)
    }
}

/// Descriptive sibling-boundary state at one point in a structural pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct BoundaryState {
    pub(crate) previous: FirstClass,
    pub(crate) pending: PendingAnchor,
}

impl BoundaryState {
    pub(crate) const START: Self = Self {
        previous: FirstClass::Empty,
        pending: PendingAnchor::None,
    };

    pub(crate) const fn new(previous: FirstClass, pending: PendingAnchor) -> Self {
        Self { previous, pending }
    }

    fn index(self) -> usize {
        self.previous.index() * PENDING_ANCHOR_COUNT + self.pending.index()
    }

    pub(crate) fn all() -> impl Iterator<Item = Self> {
        FirstClass::ALL.into_iter().flat_map(|previous| {
            PendingAnchor::ALL
                .into_iter()
                .map(move |pending| Self { previous, pending })
        })
    }

    fn tighten(self, anchor: PendingAnchor) -> Self {
        Self {
            previous: self.previous,
            pending: self.pending.tighten(anchor),
        }
    }

    fn consume(self, class: FirstClass) -> Self {
        let _ = self;
        Self {
            previous: class,
            pending: PendingAnchor::None,
        }
    }
}

/// One path-sensitive result of applying a pattern to a boundary state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct BoundaryOutcome {
    pub(crate) state: BoundaryState,
    /// Whether this pattern, rather than an earlier caller prefix, consumed a node.
    pub(crate) consumed: bool,
    /// Class of this pattern's first consumer. Kept separately from the tail
    /// because entry anchors observe the first while followers observe the last.
    pub(crate) first: FirstClass,
}

impl BoundaryOutcome {
    const fn identity(state: BoundaryState) -> Self {
        Self {
            state,
            consumed: false,
            first: FirstClass::Empty,
        }
    }
}

/// `input boundary state -> possible semantic outcomes`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BoundaryRelation {
    outcomes: [BTreeSet<BoundaryOutcome>; STATE_COUNT],
}

impl BoundaryRelation {
    fn empty() -> Self {
        Self {
            outcomes: std::array::from_fn(|_| BTreeSet::new()),
        }
    }

    pub(super) fn identity() -> Self {
        let mut relation = Self::empty();
        for state in BoundaryState::all() {
            relation
                .outcomes_mut(state)
                .insert(BoundaryOutcome::identity(state));
        }
        relation
    }

    pub(crate) fn atom(class: FirstClass) -> Self {
        let mut relation = Self::empty();
        for input in BoundaryState::all() {
            relation.outcomes_mut(input).insert(BoundaryOutcome {
                state: input.consume(class),
                consumed: true,
                first: class,
            });
        }
        relation
    }

    pub(crate) fn outcomes(&self, input: BoundaryState) -> &BTreeSet<BoundaryOutcome> {
        &self.outcomes[input.index()]
    }

    fn outcomes_mut(&mut self, input: BoundaryState) -> &mut BTreeSet<BoundaryOutcome> {
        &mut self.outcomes[input.index()]
    }

    pub(super) fn union_with(&mut self, other: &Self) {
        for input in BoundaryState::all() {
            self.outcomes_mut(input)
                .extend(other.outcomes(input).iter().copied());
        }
    }

    fn then(&self, next: &Self) -> Self {
        let mut composed = Self::empty();
        for input in BoundaryState::all() {
            for prefix in self.outcomes(input) {
                for suffix in next.outcomes(prefix.state) {
                    composed.outcomes_mut(input).insert(BoundaryOutcome {
                        state: suffix.state,
                        consumed: prefix.consumed || suffix.consumed,
                        first: if prefix.consumed {
                            prefix.first
                        } else {
                            suffix.first
                        },
                    });
                }
            }
        }
        composed
    }

    pub(crate) fn anchor(&self, anchor: PendingAnchor) -> Self {
        let mut tightened = Self::empty();
        for input in BoundaryState::all() {
            for outcome in self.outcomes(input) {
                tightened.outcomes_mut(input).insert(BoundaryOutcome {
                    state: outcome.state.tighten(anchor),
                    consumed: outcome.consumed,
                    first: outcome.first,
                });
            }
        }
        tightened
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

    fn consuming_only(&self) -> Self {
        let mut consuming = Self::empty();
        for input in BoundaryState::all() {
            consuming.outcomes_mut(input).extend(
                self.outcomes(input)
                    .iter()
                    .copied()
                    .filter(|out| out.consumed),
            );
        }
        consuming
    }

    /// Quantifier closure with empty inner outcomes pruned. An iteration that
    /// consumes nothing is never a loop edge.
    fn consuming_closure(&self, include_identity: bool) -> Self {
        let mut closure = Self::empty();
        for input in BoundaryState::all() {
            if include_identity {
                closure
                    .outcomes_mut(input)
                    .insert(BoundaryOutcome::identity(input));
            }

            let mut seen = BTreeSet::new();
            let mut pending = vec![(input, FirstClass::Empty)];
            while let Some((state, first)) = pending.pop() {
                for outcome in self.outcomes(state).iter().filter(|out| out.consumed) {
                    let first = if first == FirstClass::Empty {
                        outcome.first
                    } else {
                        first
                    };
                    if seen.insert((outcome.state, first)) {
                        pending.push((outcome.state, first));
                    }
                }
            }

            closure
                .outcomes_mut(input)
                .extend(seen.into_iter().map(|(state, first)| BoundaryOutcome {
                    state,
                    consumed: true,
                    first,
                }));
        }
        closure
    }
}

/// Computes definition relations to a least fixed point, then answers pattern
/// relation queries using those stable definition summaries.
pub(crate) struct BoundaryAnalyzer<'a> {
    interner: &'a Interner,
    symbol_table: &'a SymbolTable,
    dependencies: &'a DependencyAnalysis,
    definitions: HashMap<DefId, BoundaryRelation>,
}

impl<'a> BoundaryAnalyzer<'a> {
    pub(crate) fn new(
        interner: &'a Interner,
        symbol_table: &'a SymbolTable,
        dependencies: &'a DependencyAnalysis,
    ) -> Self {
        let mut analyzer = Self {
            interner,
            symbol_table,
            dependencies,
            definitions: HashMap::new(),
        };
        analyzer.compute_definitions();
        analyzer
    }

    pub(crate) fn pattern(&self, pattern: &Pattern) -> BoundaryRelation {
        self.compute_pattern(pattern)
    }

    pub(crate) fn items(&self, items: &[SeqItem]) -> BoundaryRelation {
        self.compute_items(items)
    }

    pub(crate) fn definition(&self, def_id: DefId) -> &BoundaryRelation {
        self.definitions
            .get(&def_id)
            .expect("every analyzed definition has a boundary relation")
    }

    fn compute_definitions(&mut self) {
        for scc in self.dependencies.sccs() {
            for &def_id in scc {
                self.definitions
                    .entry(def_id)
                    .or_insert_with(BoundaryRelation::empty);
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

    fn compute_pattern(&self, pattern: &Pattern) -> BoundaryRelation {
        match pattern {
            Pattern::NamedNodePattern(_) => BoundaryRelation::atom(FirstClass::Named),
            Pattern::AnonymousNodePattern(_) => BoundaryRelation::atom(FirstClass::Anonymous),
            Pattern::NodeWildcard(_) => BoundaryRelation::atom(FirstClass::Either),
            Pattern::CapturedPattern(capture) => capture
                .inner()
                .map_or_else(BoundaryRelation::identity, |inner| {
                    self.compute_pattern(&inner)
                }),
            Pattern::FieldPattern(field) => field
                .value()
                .map_or_else(BoundaryRelation::identity, |value| {
                    self.compute_pattern(&value)
                }),
            Pattern::SeqPattern(sequence) => {
                let items: Vec<_> = sequence.items().collect();
                self.compute_items(&items)
            }
            Pattern::Alternation(alternation) => {
                let mut relation = BoundaryRelation::empty();
                let mut had_alternative = false;
                for alternative in alternation.patterns() {
                    had_alternative = true;
                    relation.union_with(&self.compute_pattern(&alternative));
                }
                if had_alternative {
                    relation
                } else {
                    BoundaryRelation::identity()
                }
            }
            Pattern::QuantifiedPattern(quantified) => {
                let Some(inner) = quantified.inner() else {
                    return BoundaryRelation::identity();
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
                    return BoundaryRelation::empty();
                };
                self.definitions.get(&def_id).cloned().unwrap_or_else(|| {
                    panic!(
                        "boundary analysis resolved definition {def_id:?}, but its relation was \
                             not initialized before the reference was evaluated"
                    )
                })
            }
        }
    }

    fn compute_items(&self, items: &[SeqItem]) -> BoundaryRelation {
        let mut relation = BoundaryRelation::identity();
        for item in items {
            relation = match item {
                SeqItem::Anchor(anchor) => relation.anchor(if anchor.is_exact() {
                    PendingAnchor::Exact
                } else {
                    PendingAnchor::Soft
                }),
                SeqItem::Pattern(pattern) => relation.then(&self.compute_pattern(pattern)),
            };
        }
        relation
    }
}
