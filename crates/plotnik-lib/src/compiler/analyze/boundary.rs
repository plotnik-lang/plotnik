//! Path-sensitive sibling-boundary semantics.
//!
//! Lowering needs more than nullability. A structural pattern may consume a
//! named node, consume a node that must be treated conservatively by a future
//! soft anchor, or consume nothing while leaving an anchor pending. This module
//! computes that finite relation independently of bytecode and runtime ABI
//! choices.

use std::collections::BTreeSet;

use crate::compiler::parse::ast::QuantifierKind;

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
    pub(super) fn empty() -> Self {
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

    pub(super) fn then(&self, next: &Self) -> Self {
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
