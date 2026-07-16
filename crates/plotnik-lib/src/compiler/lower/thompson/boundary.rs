//! Boundary semantics translated into concrete Thompson navigation.
//!
//! Analysis describes what a pattern leaves at a sibling boundary. This module
//! is the lowering-side adapter: it intersects that state with an authored
//! entry navigation, selects the concrete VM navigation, and reconstructs the
//! state represented by an operational exit port.

use crate::bytecode::Nav;
use crate::compiler::analyze::anchors::GapClass;
use crate::compiler::analyze::boundary::{BoundaryState, FirstClass, PendingAnchor};
use crate::compiler::lower::boundary::ExitPort;
use crate::core::NodeFieldId;

/// Navigation an atom must perform when it receives control.
///
/// The wrapped navigation may already be constrained by an enclosing search.
/// Resolving the contract only tightens that constraint; a pending soft anchor
/// cannot loosen an exact or extras-only entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct NavigationContract {
    authored: Nav,
}

impl NavigationContract {
    pub(crate) fn from_nav(authored: Nav) -> Self {
        assert!(
            matches!(
                authored,
                Nav::Stay
                    | Nav::StayExact
                    | Nav::Next
                    | Nav::NextSkip
                    | Nav::NextSkipExtras
                    | Nav::NextExact
                    | Nav::Down
                    | Nav::DownSkip
                    | Nav::DownSkipExtras
                    | Nav::DownExact
            ),
            "entry navigation must stay, advance to a sibling, or descend to a child, got \
             {authored:?}"
        );
        Self { authored }
    }

    pub(crate) fn authored(self) -> Nav {
        self.authored
    }

    /// Resolve this entry for the next node-consuming pattern.
    pub(crate) fn resolve(self, state: BoundaryState, next: FirstClass) -> Nav {
        assert!(
            next != FirstClass::Empty,
            "entry navigation can only be resolved for a consuming pattern"
        );

        let boundary_gap = gap_before_next(state, next);
        match self.authored {
            Nav::Stay => {
                if boundary_gap == GapClass::Any {
                    Nav::Stay
                } else {
                    Nav::StayExact
                }
            }
            Nav::StayExact => Nav::StayExact,
            nav @ (Nav::Next | Nav::NextSkip | Nav::NextSkipExtras | Nav::NextExact) => {
                next_nav(authored_gap(nav).tighten(boundary_gap))
            }
            nav @ (Nav::Down | Nav::DownSkip | Nav::DownSkipExtras | Nav::DownExact) => {
                down_nav(authored_gap(nav).tighten(boundary_gap))
            }
            _ => unreachable!("NavigationContract validates its authored navigation"),
        }
    }
}

/// Everything that must reach the first actual consumer of a structural form.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct EntryObligation {
    pub(crate) navigation: NavigationContract,
    pub(crate) field: Option<NodeFieldId>,
}

impl EntryObligation {
    pub(crate) fn new(navigation: NavigationContract) -> Self {
        Self {
            navigation,
            field: None,
        }
    }

    pub(crate) fn with_field(mut self, field: NodeFieldId) -> Self {
        assert!(
            self.field.is_none(),
            "an entry obligation must discharge at most one grammar field"
        );
        self.field = Some(field);
        self
    }

    pub(crate) fn navigation(self) -> NavigationContract {
        self.navigation
    }

    pub(crate) fn field(self) -> Option<NodeFieldId> {
        self.field
    }

    pub(crate) fn resolve_nav(self, state: BoundaryState, next: FirstClass) -> Nav {
        self.navigation.resolve(state, next)
    }
}

/// Navigation for leaving a consumed child list.
///
/// The enclosing boundary is operationally named. A soft trailing anchor is
/// therefore broad only after a named (or absent) previous consumer.
pub(crate) fn trailing_up_nav(state: BoundaryState, levels: u8) -> Nav {
    assert!(
        (1..=Nav::MAX_UP_LEVEL).contains(&levels),
        "trailing ascent level must fit one Up instruction"
    );

    match gap_before_next(state, FirstClass::Named) {
        GapClass::Any => Nav::Up(levels),
        GapClass::AnonymousAndExtras => Nav::UpSkipTrivia(levels),
        GapClass::ExtrasOnly => Nav::UpSkipExtras(levels),
        GapClass::Exact => Nav::UpExact(levels),
    }
}

/// Empty-list counterpart of [`trailing_up_nav`].
///
/// No check is necessary without a pending anchor. Otherwise the childless
/// instruction enforces the same gap class without descending or ascending.
pub(crate) fn trailing_childless_nav(state: BoundaryState) -> Option<Nav> {
    let nav = match gap_before_next(state, FirstClass::Named) {
        GapClass::Any => return None,
        GapClass::AnonymousAndExtras => Nav::ChildlessSkipTrivia,
        GapClass::ExtrasOnly => Nav::ChildlessSkipExtras,
        GapClass::Exact => Nav::ChildlessExact,
    };
    Some(nav)
}

/// Reconstruct the canonical boundary state represented by an exit port.
///
/// Empty outcomes retain the caller's previous consumer. The port already
/// names the relation's final pending-anchor state; it must not be recomposed
/// with the input a second time.
pub(crate) fn next_boundary_state(input: BoundaryState, port: ExitPort) -> BoundaryState {
    match port {
        ExitPort::ConsumedNamedNone => BoundaryState::new(FirstClass::Named, PendingAnchor::None),
        ExitPort::ConsumedOtherNone => {
            BoundaryState::new(FirstClass::Anonymous, PendingAnchor::None)
        }
        ExitPort::ConsumedNamedSoft => BoundaryState::new(FirstClass::Named, PendingAnchor::Soft),
        ExitPort::ConsumedOtherSoft => {
            BoundaryState::new(FirstClass::Anonymous, PendingAnchor::Soft)
        }
        ExitPort::ConsumedExact => BoundaryState::new(FirstClass::Anonymous, PendingAnchor::Exact),
        ExitPort::EmptyNone => BoundaryState::new(input.previous, PendingAnchor::None),
        ExitPort::EmptySoft => BoundaryState::new(input.previous, PendingAnchor::Soft),
        ExitPort::EmptyExact => BoundaryState::new(input.previous, PendingAnchor::Exact),
    }
}

fn gap_before_next(state: BoundaryState, next: FirstClass) -> GapClass {
    match state.pending {
        PendingAnchor::None => GapClass::Any,
        PendingAnchor::Exact => GapClass::Exact,
        PendingAnchor::Soft => {
            let previous_allows_broad =
                matches!(state.previous, FirstClass::Empty | FirstClass::Named);
            if previous_allows_broad && next == FirstClass::Named {
                GapClass::AnonymousAndExtras
            } else {
                GapClass::ExtrasOnly
            }
        }
    }
}

fn authored_gap(nav: Nav) -> GapClass {
    GapClass::from_nav(nav).expect("Next and Down navigation always describe a sibling gap")
}

fn next_nav(gap: GapClass) -> Nav {
    match gap {
        GapClass::Any => Nav::Next,
        GapClass::AnonymousAndExtras => Nav::NextSkip,
        GapClass::ExtrasOnly => Nav::NextSkipExtras,
        GapClass::Exact => Nav::NextExact,
    }
}

fn down_nav(gap: GapClass) -> Nav {
    match gap {
        GapClass::Any => Nav::Down,
        GapClass::AnonymousAndExtras => Nav::DownSkip,
        GapClass::ExtrasOnly => Nav::DownSkipExtras,
        GapClass::Exact => Nav::DownExact,
    }
}
