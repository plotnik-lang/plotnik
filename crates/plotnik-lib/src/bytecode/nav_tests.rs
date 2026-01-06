use super::*;

#[test]
fn nav_standard_roundtrip() {
    for nav in [
        Nav::Stay,
        Nav::StayExact,
        Nav::Next,
        Nav::NextSkip,
        Nav::NextExact,
        Nav::Down,
        Nav::DownSkip,
        Nav::DownExact,
    ] {
        assert_eq!(Nav::from_byte(nav.to_byte()), nav);
    }
}

#[test]
fn nav_up_roundtrip() {
    let nav = Nav::Up(5);
    assert_eq!(Nav::from_byte(nav.to_byte()), nav);

    let nav = Nav::UpSkipTrivia(10);
    assert_eq!(Nav::from_byte(nav.to_byte()), nav);

    let nav = Nav::UpExact(63);
    assert_eq!(Nav::from_byte(nav.to_byte()), nav);
}

#[test]
fn nav_byte_encoding() {
    assert_eq!(Nav::Stay.to_byte(), 0b00_000000);
    assert_eq!(Nav::StayExact.to_byte(), 0b00_000001);
    assert_eq!(Nav::Down.to_byte(), 0b00_000101);
    assert_eq!(Nav::Up(5).to_byte(), 0b01_000101);
    assert_eq!(Nav::UpSkipTrivia(3).to_byte(), 0b10_000011);
    assert_eq!(Nav::UpExact(1).to_byte(), 0b11_000001);
}

#[test]
#[should_panic(expected = "invalid nav standard")]
fn nav_invalid_standard_panics() {
    Nav::from_byte(0b00_111111);
}

#[test]
#[should_panic(expected = "invalid nav up level")]
fn nav_invalid_up_zero_panics() {
    Nav::from_byte(0b01_000000);
}
