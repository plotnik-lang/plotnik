use crate::bytecode::Nav;
use crate::compiler::analyze::anchors::GapClass;
use crate::compiler::analyze::boundary::ExitPort;
use crate::compiler::analyze::boundary::{BoundaryState, FirstClass, PendingAnchor};
use crate::core::NodeFieldId;

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

    pub(crate) fn resolve(self, state: BoundaryState, next: FirstClass) -> Nav {
        assert!(
            next != FirstClass::Empty,
            "entry navigation can only be resolved for a consuming pattern"
        );

        let boundary_gap = state.gap_before(next);
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

pub(crate) fn trailing_up_nav(state: BoundaryState, levels: u8) -> Nav {
    assert!(
        (1..=Nav::MAX_UP_LEVEL).contains(&levels),
        "trailing ascent level must fit one Up instruction"
    );

    match state.gap_before(FirstClass::Named) {
        GapClass::Any => Nav::Up(levels),
        GapClass::AnonymousAndExtras => Nav::UpSkipTrivia(levels),
        GapClass::ExtrasOnly => Nav::UpSkipExtras(levels),
        GapClass::Exact => Nav::UpExact(levels),
    }
}

pub(crate) fn trailing_childless_nav(state: BoundaryState) -> Option<Nav> {
    let nav = match state.gap_before(FirstClass::Named) {
        GapClass::Any => return None,
        GapClass::AnonymousAndExtras => Nav::ChildlessSkipTrivia,
        GapClass::ExtrasOnly => Nav::ChildlessSkipExtras,
        GapClass::Exact => Nav::ChildlessExact,
    };
    Some(nav)
}

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
