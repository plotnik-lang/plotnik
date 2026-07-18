//! Operational quotient of descriptive boundary outcomes.
//!
//! Lowering routes only distinctions that future matching can observe. In
//! particular, an anonymous consumer and an any-node consumer share one
//! conservative class, and exact pending anchors make previous namedness
//! irrelevant.

use std::collections::BTreeMap;

use crate::compiler::analyze::boundary::{
    BoundaryOutcome, BoundaryState, FirstClass, PendingAnchor,
};
use plotnik_rt::PortId;

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
