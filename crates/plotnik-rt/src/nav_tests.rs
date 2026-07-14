use super::*;

#[test]
fn nav_standard_roundtrip() {
    for nav in [
        Nav::Epsilon,
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
        Nav::ChildlessSkipTrivia,
        Nav::ChildlessSkipExtras,
        Nav::ChildlessExact,
    ] {
        assert_eq!(Nav::from_byte(nav.to_byte()), nav);
    }
}

#[test]
fn nav_up_roundtrip() {
    for nav in [
        Nav::Up(1),
        Nav::Up(5),
        Nav::UpSkipTrivia(10),
        Nav::UpSkipExtras(Nav::MAX_UP_LEVEL),
        Nav::UpExact(Nav::MAX_UP_LEVEL),
    ] {
        assert_eq!(Nav::from_byte(nav.to_byte()), nav);
    }
}

#[test]
fn nav_byte_encoding() {
    // Standard commands: bit 7 clear, value in bits 6-0.
    assert_eq!(Nav::Epsilon.to_byte(), 0b0000_0000);
    assert_eq!(Nav::Stay.to_byte(), 0b0000_0001);
    assert_eq!(Nav::StayExact.to_byte(), 0b0000_0010);
    assert_eq!(Nav::NextSkipExtras.to_byte(), 0b0000_0101);
    assert_eq!(Nav::Down.to_byte(), 0b0000_0111);
    assert_eq!(Nav::DownSkipExtras.to_byte(), 0b0000_1001);
    assert_eq!(Nav::ChildlessSkipTrivia.to_byte(), 0b0000_1011);
    assert_eq!(Nav::ChildlessExact.to_byte(), 0b0000_1101);

    // Up family: bit 7 set, then 2-bit mode (bits 6-5), then 5-bit level (bits 4-0).
    assert_eq!(Nav::Up(5).to_byte(), 0b1000_0101);
    assert_eq!(Nav::UpSkipTrivia(3).to_byte(), 0b1010_0011);
    assert_eq!(Nav::UpSkipExtras(3).to_byte(), 0b1100_0011);
    assert_eq!(Nav::UpExact(1).to_byte(), 0b1110_0001);
}

#[test]
#[should_panic(expected = "Up level overflow")]
fn nav_up_level_overflow_panics() {
    // Every Up mode shares the 5-bit level field, so 32 overflows uniformly.
    Nav::UpSkipExtras(Nav::MAX_UP_LEVEL + 1).to_byte();
}

#[test]
#[should_panic(expected = "invalid nav byte")]
fn nav_invalid_up_zero_panics() {
    // Up family (bit 7) with a zero level is not a valid command.
    Nav::from_byte(0b1000_0000);
}

#[test]
fn nav_reserved_standard_byte_rejected() {
    // Standard enum values 14..=127 (bit 7 clear) are unassigned.
    assert_eq!(Nav::try_from_byte(0b0000_1110), None);
    assert_eq!(Nav::try_from_byte(0b0111_1111), None);
}

#[test]
fn nav_up_accessors() {
    // up_level reports the level for any Up nav, None otherwise.
    assert_eq!(Nav::Up(7).up_level(), Some(7));
    assert_eq!(
        Nav::UpExact(Nav::MAX_UP_LEVEL).up_level(),
        Some(Nav::MAX_UP_LEVEL)
    );
    assert_eq!(Nav::Next.up_level(), None);

    // up_mode_tag is the 2-bit mode (matching bits 6-5 of the encoded byte).
    assert_eq!(Nav::Up(1).up_mode_tag(), Some(0b00));
    assert_eq!(Nav::UpSkipTrivia(1).up_mode_tag(), Some(0b01));
    assert_eq!(Nav::UpSkipExtras(1).up_mode_tag(), Some(0b10));
    assert_eq!(Nav::UpExact(1).up_mode_tag(), Some(0b11));
    assert_eq!(Nav::Down.up_mode_tag(), None);
}

#[test]
fn nav_same_up_mode() {
    assert!(Nav::Up(1).same_up_mode(Nav::Up(9)));
    assert!(Nav::UpExact(2).same_up_mode(Nav::UpExact(5)));
    assert!(!Nav::Up(1).same_up_mode(Nav::UpExact(1)));
    // Non-Up navs are never the same Up mode, not even with themselves.
    assert!(!Nav::Next.same_up_mode(Nav::Next));
    assert!(!Nav::Up(1).same_up_mode(Nav::Next));
}

#[test]
fn nav_with_up_level_preserves_mode() {
    assert_eq!(Nav::Up(1).with_up_level(9), Nav::Up(9));
    assert_eq!(Nav::UpSkipTrivia(2).with_up_level(5), Nav::UpSkipTrivia(5));
    assert_eq!(Nav::UpSkipExtras(2).with_up_level(5), Nav::UpSkipExtras(5));
    assert_eq!(Nav::UpExact(2).with_up_level(5), Nav::UpExact(5));
}

#[test]
#[should_panic(expected = "with_up_level on non-Up nav")]
fn nav_with_up_level_on_non_up_panics() {
    Nav::Next.with_up_level(2);
}

#[test]
fn nav_skip_policy_mapping() {
    assert_eq!(Nav::Down.skip_policy(), SkipPolicy::Any);
    assert_eq!(Nav::Next.skip_policy(), SkipPolicy::Any);
    assert_eq!(Nav::Stay.skip_policy(), SkipPolicy::Any);
    assert_eq!(Nav::DownSkip.skip_policy(), SkipPolicy::Trivia);
    assert_eq!(Nav::NextSkip.skip_policy(), SkipPolicy::Trivia);
    assert_eq!(Nav::DownSkipExtras.skip_policy(), SkipPolicy::Extras);
    assert_eq!(Nav::NextSkipExtras.skip_policy(), SkipPolicy::Extras);
    assert_eq!(Nav::DownExact.skip_policy(), SkipPolicy::Exact);
    assert_eq!(Nav::NextExact.skip_policy(), SkipPolicy::Exact);
    assert_eq!(Nav::StayExact.skip_policy(), SkipPolicy::Exact);
    assert_eq!(Nav::ChildlessExact.skip_policy(), SkipPolicy::Exact);
    assert_eq!(Nav::Up(2).skip_policy(), SkipPolicy::Any);
}

/// The engine-owned searches are exactly the non-exact `Down*`/`Next*` navs.
/// Two invariants hang off this set: acceptance there pushes a match-retry
/// checkpoint, and lowering's `to_exact` (applied to every NFA-loop-internal
/// navigation state) must map every member out of the set — otherwise a search would have
/// two retry owners and backtracking would enumerate candidates twice.
#[test]
fn nav_sibling_search_set() {
    let searching = [
        Nav::Down,
        Nav::DownSkip,
        Nav::DownSkipExtras,
        Nav::Next,
        Nav::NextSkip,
        Nav::NextSkipExtras,
    ];
    for nav in searching {
        assert!(nav.is_sibling_search(), "{nav:?}");
        assert!(!nav.to_exact().is_sibling_search(), "{nav:?}");
        assert_ne!(nav.skip_policy(), SkipPolicy::Exact, "{nav:?}");
    }

    for nav in [
        Nav::Epsilon,
        Nav::Stay,
        Nav::StayExact,
        Nav::DownExact,
        Nav::NextExact,
        Nav::ChildlessSkipTrivia,
        Nav::ChildlessSkipExtras,
        Nav::ChildlessExact,
        Nav::Up(1),
        Nav::UpSkipTrivia(1),
        Nav::UpSkipExtras(1),
        Nav::UpExact(1),
    ] {
        assert!(!nav.is_sibling_search(), "{nav:?}");
    }
}
