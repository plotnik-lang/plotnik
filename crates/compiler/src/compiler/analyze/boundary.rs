use std::collections::BTreeSet;

use super::anchors::GapClass;

use crate::compiler::parse::ast::QuantifierKind;

const FIRST_CLASS_COUNT: usize = 4;
const PENDING_ANCHOR_COUNT: usize = 3;
const STATE_COUNT: usize = FIRST_CLASS_COUNT * PENDING_ANCHOR_COUNT;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum PendingAnchor {
    None,
    Soft,
    Exact,
}

impl PendingAnchor {
    pub(crate) const ALL: [Self; PENDING_ANCHOR_COUNT] = [Self::None, Self::Soft, Self::Exact];

    pub(crate) fn index(self) -> usize {
        match self {
            Self::None => 0,
            Self::Soft => 1,
            Self::Exact => 2,
        }
    }

    pub(crate) fn tighten(self, other: Self) -> Self {
        std::cmp::max(self, other)
    }
}

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

    pub(crate) const COUNT: usize = STATE_COUNT;

    pub(crate) fn from_index(index: usize) -> Self {
        Self::new(
            FirstClass::ALL[index / PENDING_ANCHOR_COUNT],
            PendingAnchor::ALL[index % PENDING_ANCHOR_COUNT],
        )
    }

    pub(crate) fn index(self) -> usize {
        self.previous.index() * PENDING_ANCHOR_COUNT + self.pending.index()
    }

    pub(crate) fn all() -> impl Iterator<Item = Self> {
        FirstClass::ALL.into_iter().flat_map(|previous| {
            PendingAnchor::ALL
                .into_iter()
                .map(move |pending| Self { previous, pending })
        })
    }

    pub(crate) fn tighten(self, anchor: PendingAnchor) -> Self {
        Self {
            previous: self.previous,
            pending: self.pending.tighten(anchor),
        }
    }

    pub(crate) fn gap_before(self, next: FirstClass) -> GapClass {
        match self.pending {
            PendingAnchor::None => GapClass::Any,
            PendingAnchor::Exact => GapClass::Exact,
            PendingAnchor::Soft => {
                let previous_allows_broad =
                    matches!(self.previous, FirstClass::Empty | FirstClass::Named);
                if previous_allows_broad && next == FirstClass::Named {
                    GapClass::AnonymousAndExtras
                } else {
                    GapClass::ExtrasOnly
                }
            }
        }
    }

    pub(crate) fn consume(self, class: FirstClass) -> Self {
        let _ = self;
        Self {
            previous: class,
            pending: PendingAnchor::None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct BoundaryOutcome {
    pub(crate) state: BoundaryState,
    pub(crate) consumed: bool,
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

    pub(crate) fn repeat_entry_gap(&self, input: BoundaryState) -> Option<GapClass> {
        let outcomes: Vec<_> = self.outcomes(input).iter().copied().collect();
        if !simple_boundary_outcomes(&outcomes) {
            return None;
        }
        let first = outcomes.iter().find(|outcome| outcome.consumed)?.first;
        let gap = input.gap_before(first);
        (gap != GapClass::Any).then_some(gap)
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum ExitPort {
    ConsumedNamedNone,
    ConsumedOtherNone,
    ConsumedNamedSoft,
    ConsumedOtherSoft,
    ConsumedExact,
    EmptyNone,
    EmptySoft,
    EmptyExact,
}

impl ExitPort {
    pub(crate) fn from_outcome(outcome: BoundaryOutcome) -> Self {
        if !outcome.consumed {
            return match outcome.state.pending {
                PendingAnchor::None => Self::EmptyNone,
                PendingAnchor::Soft => Self::EmptySoft,
                PendingAnchor::Exact => Self::EmptyExact,
            };
        }

        match (outcome.state.previous, outcome.state.pending) {
            (_, PendingAnchor::Exact) => Self::ConsumedExact,
            (FirstClass::Named, PendingAnchor::None) => Self::ConsumedNamedNone,
            (FirstClass::Named, PendingAnchor::Soft) => Self::ConsumedNamedSoft,
            (FirstClass::Anonymous | FirstClass::Either, PendingAnchor::None) => {
                Self::ConsumedOtherNone
            }
            (FirstClass::Anonymous | FirstClass::Either, PendingAnchor::Soft) => {
                Self::ConsumedOtherSoft
            }
            (FirstClass::Empty, _) => {
                unreachable!("a consuming outcome has a previous consumer")
            }
        }
    }

    pub(crate) fn from_state(state: BoundaryState, consumed: bool) -> Self {
        Self::from_outcome(BoundaryOutcome {
            state,
            consumed,
            first: if consumed {
                state.previous
            } else {
                FirstClass::Empty
            },
        })
    }

    pub(crate) fn consumed(self) -> bool {
        !matches!(self, Self::EmptyNone | Self::EmptySoft | Self::EmptyExact)
    }
}

pub(crate) fn simple_boundary_outcomes(outcomes: &[BoundaryOutcome]) -> bool {
    let mut consuming = BTreeSet::new();
    let mut empty = BTreeSet::new();
    for outcome in outcomes {
        let port = ExitPort::from_outcome(*outcome);
        if port.consumed() {
            consuming.insert((port, outcome.first));
        } else {
            empty.insert(port);
        }
    }
    consuming.len() <= 1 && empty.len() <= 1
}
