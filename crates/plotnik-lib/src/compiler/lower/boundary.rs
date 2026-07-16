//! Operational quotient of descriptive boundary outcomes.
//!
//! Lowering routes only distinctions that future matching can observe. In
//! particular, an anonymous consumer and an any-node consumer share one
//! conservative class, and exact pending anchors make previous namedness
//! irrelevant.

use std::collections::BTreeMap;

#[cfg(test)]
use crate::compiler::analyze::anchors::GapClass;
#[cfg(test)]
use crate::compiler::analyze::boundary::BoundaryRelation;
use crate::compiler::analyze::boundary::{
    BoundaryOutcome, BoundaryState, FirstClass, PendingAnchor,
};
use plotnik_rt::PortId;

/// Previous-consumer distinction retained by executable continuations.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OperationalPrevious {
    Empty,
    Named,
    Other,
}

/// Executable boundary state reconstructed at a continuation.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OperationalState {
    previous: OperationalPrevious,
    pending: PendingAnchor,
}

#[cfg(test)]
impl OperationalState {
    fn from_semantic(state: BoundaryState) -> Self {
        Self {
            previous: match state.previous {
                FirstClass::Empty => OperationalPrevious::Empty,
                FirstClass::Named => OperationalPrevious::Named,
                FirstClass::Anonymous | FirstClass::Either => OperationalPrevious::Other,
            },
            pending: state.pending,
        }
    }

    fn tighten(self, anchor: PendingAnchor) -> Self {
        Self {
            previous: self.previous,
            pending: self.pending.tighten(anchor),
        }
    }

    fn observes(self, next: FirstClass) -> GapClass {
        match self.pending {
            PendingAnchor::None => GapClass::Any,
            PendingAnchor::Exact => GapClass::Exact,
            PendingAnchor::Soft => {
                let previous_allows_broad = matches!(
                    self.previous,
                    OperationalPrevious::Empty | OperationalPrevious::Named
                );
                if previous_allows_broad && next == FirstClass::Named {
                    GapClass::AnonymousAndExtras
                } else {
                    GapClass::ExtrasOnly
                }
            }
        }
    }
}

/// Maximum semantic continuation universe after operationally equivalent
/// descriptive outcomes are merged.
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

    #[cfg(test)]
    fn apply(self, input: OperationalState) -> OperationalState {
        match self {
            Self::ConsumedNamedNone => OperationalState {
                previous: OperationalPrevious::Named,
                pending: PendingAnchor::None,
            },
            Self::ConsumedOtherNone => OperationalState {
                previous: OperationalPrevious::Other,
                pending: PendingAnchor::None,
            },
            Self::ConsumedNamedSoft => OperationalState {
                previous: OperationalPrevious::Named,
                pending: PendingAnchor::Soft,
            },
            Self::ConsumedOtherSoft => OperationalState {
                previous: OperationalPrevious::Other,
                pending: PendingAnchor::Soft,
            },
            // Previous namedness is unobservable until this exact anchor is
            // discharged, so choose one canonical representative.
            Self::ConsumedExact => OperationalState {
                previous: OperationalPrevious::Other,
                pending: PendingAnchor::Exact,
            },
            Self::EmptyNone => OperationalState {
                previous: input.previous,
                pending: PendingAnchor::None,
            },
            Self::EmptySoft => OperationalState {
                previous: input.previous,
                pending: PendingAnchor::Soft,
            },
            Self::EmptyExact => OperationalState {
                previous: input.previous,
                pending: PendingAnchor::Exact,
            },
        }
    }
}

/// Exact ordered port set exposed by one lowered specialization.
///
/// Ordering is semantic `ExitPort` order, so equal contracts share one dense
/// local numbering independent of construction order.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ExitSignature {
    ports: Box<[ExitPort]>,
}

impl ExitSignature {
    pub(crate) fn from_ports(ports: impl IntoIterator<Item = ExitPort>) -> Self {
        let ports = ports
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .into_boxed_slice();
        assert!(
            !ports.is_empty(),
            "an exit signature exposes at least one port"
        );
        assert!(
            ports.len() <= usize::from(PortId::COUNT),
            "operational exit signature exceeds the maximum port universe"
        );
        Self { ports }
    }

    #[cfg(test)]
    pub(crate) fn from_relation(relation: &BoundaryRelation, input: BoundaryState) -> Self {
        Self::from_ports(
            relation
                .outcomes(input)
                .iter()
                .copied()
                .map(ExitPort::from_outcome),
        )
    }

    pub(crate) fn singleton(port: ExitPort) -> Self {
        Self::from_ports([port])
    }

    pub(crate) fn ports(&self) -> &[ExitPort] {
        &self.ports
    }

    pub(crate) fn port_id(&self, port: ExitPort) -> Option<PortId> {
        let index = self.ports.binary_search(&port).ok()?;
        PortId::new(u8::try_from(index).expect("an exit signature has at most eight ports"))
    }

    pub(crate) fn len(&self) -> usize {
        self.ports.len()
    }
}

/// Dense-by-signature continuation map. Missing ports mean failure; callers
/// must never substitute a canonical continuation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExitMap<T> {
    targets: BTreeMap<ExitPort, T>,
}

impl<T> ExitMap<T> {
    pub(crate) fn new() -> Self {
        Self {
            targets: BTreeMap::new(),
        }
    }

    pub(crate) fn insert(&mut self, port: ExitPort, target: T) -> Option<T> {
        self.targets.insert(port, target)
    }

    pub(crate) fn get(&self, port: ExitPort) -> Option<&T> {
        self.targets.get(&port)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (ExitPort, &T)> {
        self.targets.iter().map(|(&port, target)| (port, target))
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operational_quotient_preserves_every_future_anchor_observation() {
        for input in BoundaryState::all() {
            let operational_input = OperationalState::from_semantic(input);
            for previous in FirstClass::ALL {
                for pending in PendingAnchor::ALL {
                    for consumed in [false, true] {
                        if consumed && previous == FirstClass::Empty {
                            continue;
                        }

                        let semantic = BoundaryOutcome {
                            state: BoundaryState::new(previous, pending),
                            consumed,
                            first: if consumed {
                                previous
                            } else {
                                FirstClass::Empty
                            },
                        };
                        let port = ExitPort::from_outcome(semantic);
                        let quotient = port.apply(operational_input);
                        let descriptive = if consumed {
                            OperationalState::from_semantic(semantic.state)
                        } else {
                            OperationalState {
                                previous: operational_input.previous,
                                pending: semantic.state.pending,
                            }
                        };

                        for added_anchor in PendingAnchor::ALL {
                            for next in
                                [FirstClass::Named, FirstClass::Anonymous, FirstClass::Either]
                            {
                                assert_eq!(
                                    quotient.tighten(added_anchor).observes(next),
                                    descriptive.tighten(added_anchor).observes(next),
                                    "input={input:?} outcome={semantic:?} port={port:?} \
                                     added_anchor={added_anchor:?} next={next:?}",
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn exit_map_has_no_implicit_fallback() {
        let mut exits = ExitMap::new();
        exits.insert(ExitPort::ConsumedNamedNone, 7);

        assert_eq!(exits.get(ExitPort::ConsumedNamedNone), Some(&7));
        assert_eq!(exits.get(ExitPort::ConsumedOtherNone), None);
    }

    #[test]
    fn signatures_number_only_reachable_ports_densely() {
        let relation = BoundaryRelation::atom(FirstClass::Named).anchor(PendingAnchor::Soft);
        let signature = ExitSignature::from_relation(&relation, BoundaryState::START);

        assert_eq!(signature.ports(), &[ExitPort::ConsumedNamedSoft]);
        assert_eq!(
            signature.port_id(ExitPort::ConsumedNamedSoft),
            PortId::new(0),
        );
        assert_eq!(signature.port_id(ExitPort::EmptySoft), None);
    }
}
