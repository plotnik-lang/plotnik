use crate::bytecode::Nav;
use crate::compiler::analyze::boundary::{BoundaryState, FirstClass, PendingAnchor};
use crate::compiler::lower::boundary::ExitPort;
use crate::core::NodeFieldId;

use super::boundary::{
    EntryObligation, NavigationContract, next_boundary_state, trailing_childless_nav,
    trailing_up_nav,
};

fn state(previous: FirstClass, pending: PendingAnchor) -> BoundaryState {
    BoundaryState::new(previous, pending)
}

fn resolve(authored: Nav, state: BoundaryState, next: FirstClass) -> Nav {
    NavigationContract::from_nav(authored).resolve(state, next)
}

#[test]
fn soft_entry_distinguishes_broad_named_boundaries_from_conservative_ones() {
    assert_eq!(
        resolve(
            Nav::Down,
            state(FirstClass::Empty, PendingAnchor::Soft),
            FirstClass::Named,
        ),
        Nav::DownSkip,
    );
    assert_eq!(
        resolve(
            Nav::Next,
            state(FirstClass::Named, PendingAnchor::Soft),
            FirstClass::Named,
        ),
        Nav::NextSkip,
    );

    for (previous, next) in [
        (FirstClass::Anonymous, FirstClass::Named),
        (FirstClass::Either, FirstClass::Named),
        (FirstClass::Named, FirstClass::Anonymous),
        (FirstClass::Named, FirstClass::Either),
        (FirstClass::Empty, FirstClass::Anonymous),
    ] {
        assert_eq!(
            resolve(Nav::Next, state(previous, PendingAnchor::Soft), next),
            Nav::NextSkipExtras,
            "previous={previous:?} next={next:?}",
        );
    }
}

#[test]
fn unanchored_entries_preserve_their_authored_navigation() {
    for nav in [
        Nav::Stay,
        Nav::StayExact,
        Nav::Next,
        Nav::NextSkip,
        Nav::NextSkipExtras,
        Nav::NextExact,
        Nav::Down,
        Nav::DownSkip,
        Nav::DownSkipExtras,
        Nav::DownExact,
    ] {
        assert_eq!(
            resolve(
                nav,
                state(FirstClass::Named, PendingAnchor::None),
                FirstClass::Named,
            ),
            nav,
        );
    }
}

#[test]
fn resolving_an_entry_only_tightens_its_authored_constraint() {
    let soft_named = state(FirstClass::Named, PendingAnchor::Soft);
    let exact = state(FirstClass::Named, PendingAnchor::Exact);

    assert_eq!(
        resolve(Nav::NextSkipExtras, soft_named, FirstClass::Named),
        Nav::NextSkipExtras,
    );
    assert_eq!(
        resolve(Nav::NextExact, soft_named, FirstClass::Named),
        Nav::NextExact,
    );
    assert_eq!(
        resolve(Nav::NextSkip, exact, FirstClass::Named),
        Nav::NextExact,
    );
    assert_eq!(
        resolve(Nav::DownExact, soft_named, FirstClass::Named),
        Nav::DownExact,
    );
}

#[test]
fn anchored_stay_entries_are_exact_positions() {
    assert_eq!(
        resolve(
            Nav::Stay,
            state(FirstClass::Named, PendingAnchor::None),
            FirstClass::Named,
        ),
        Nav::Stay,
    );
    assert_eq!(
        resolve(
            Nav::Stay,
            state(FirstClass::Named, PendingAnchor::Soft),
            FirstClass::Named,
        ),
        Nav::StayExact,
    );
    assert_eq!(
        resolve(
            Nav::StayExact,
            state(FirstClass::Named, PendingAnchor::None),
            FirstClass::Named,
        ),
        Nav::StayExact,
    );
}

#[test]
fn entry_obligation_carries_field_to_the_first_consumer() {
    let field = NodeFieldId::try_from(7).expect("test field id is non-zero");
    let obligation =
        EntryObligation::new(NavigationContract::from_nav(Nav::Down)).with_field(field);

    assert_eq!(obligation.field, Some(field));
    assert_eq!(
        obligation.resolve_nav(
            state(FirstClass::Empty, PendingAnchor::Exact),
            FirstClass::Named,
        ),
        Nav::DownExact,
    );
}

#[test]
fn trailing_navigation_uses_the_tail_class_and_pending_anchor() {
    assert_eq!(
        trailing_up_nav(state(FirstClass::Named, PendingAnchor::None), 2),
        Nav::Up(2),
    );
    assert_eq!(
        trailing_up_nav(state(FirstClass::Empty, PendingAnchor::Soft), 1),
        Nav::UpSkipTrivia(1),
    );
    assert_eq!(
        trailing_up_nav(state(FirstClass::Named, PendingAnchor::Soft), 1),
        Nav::UpSkipTrivia(1),
    );
    assert_eq!(
        trailing_up_nav(state(FirstClass::Anonymous, PendingAnchor::Soft), 1),
        Nav::UpSkipExtras(1),
    );
    assert_eq!(
        trailing_up_nav(state(FirstClass::Either, PendingAnchor::Exact), 1),
        Nav::UpExact(1),
    );
}

#[test]
fn childless_navigation_is_the_empty_counterpart_of_trailing_navigation() {
    assert_eq!(
        trailing_childless_nav(state(FirstClass::Empty, PendingAnchor::None)),
        None,
    );
    assert_eq!(
        trailing_childless_nav(state(FirstClass::Empty, PendingAnchor::Soft)),
        Some(Nav::ChildlessSkipTrivia),
    );
    assert_eq!(
        trailing_childless_nav(state(FirstClass::Anonymous, PendingAnchor::Soft)),
        Some(Nav::ChildlessSkipExtras),
    );
    assert_eq!(
        trailing_childless_nav(state(FirstClass::Named, PendingAnchor::Exact)),
        Some(Nav::ChildlessExact),
    );
}

#[test]
fn consumed_ports_replace_the_input_state() {
    let input = state(FirstClass::Either, PendingAnchor::Exact);

    for (port, expected) in [
        (
            ExitPort::ConsumedNamedNone,
            state(FirstClass::Named, PendingAnchor::None),
        ),
        (
            ExitPort::ConsumedOtherNone,
            state(FirstClass::Anonymous, PendingAnchor::None),
        ),
        (
            ExitPort::ConsumedNamedSoft,
            state(FirstClass::Named, PendingAnchor::Soft),
        ),
        (
            ExitPort::ConsumedOtherSoft,
            state(FirstClass::Anonymous, PendingAnchor::Soft),
        ),
        (
            ExitPort::ConsumedExact,
            state(FirstClass::Anonymous, PendingAnchor::Exact),
        ),
    ] {
        assert_eq!(next_boundary_state(input, port), expected, "port={port:?}");
    }
}

#[test]
fn empty_ports_preserve_previous_and_restore_their_final_pending_anchor() {
    let soft_input = state(FirstClass::Named, PendingAnchor::Soft);
    let exact_input = state(FirstClass::Either, PendingAnchor::Exact);

    assert_eq!(
        next_boundary_state(soft_input, ExitPort::EmptyNone),
        state(FirstClass::Named, PendingAnchor::None),
    );
    assert_eq!(
        next_boundary_state(soft_input, ExitPort::EmptyExact),
        state(FirstClass::Named, PendingAnchor::Exact),
    );
    assert_eq!(
        next_boundary_state(exact_input, ExitPort::EmptySoft),
        state(FirstClass::Either, PendingAnchor::Soft),
    );
}

#[test]
#[should_panic(expected = "entry navigation must stay, advance to a sibling, or descend")]
fn navigation_contract_rejects_non_entry_navigation() {
    let _ = NavigationContract::from_nav(Nav::Up(1));
}
