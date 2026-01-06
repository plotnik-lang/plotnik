use super::*;

#[test]
fn test_symbol_format() {
    assert_eq!(Symbol::EMPTY.format(), "     ");
    assert_eq!(Symbol::EPSILON.format(), "  ε  ");
    assert_eq!(nav_symbol(Nav::StayExact).format(), "  !  ");
    assert_eq!(nav_symbol(Nav::Down).format(), "  ▽  ");
    assert_eq!(nav_symbol(Nav::DownSkip).format(), " !▽  ");
    assert_eq!(nav_symbol(Nav::DownExact).format(), "!!▽  ");
    assert_eq!(nav_symbol(Nav::Next).format(), "  ▷  ");
    assert_eq!(nav_symbol(Nav::NextSkip).format(), " !▷  ");
    assert_eq!(nav_symbol(Nav::NextExact).format(), "!!▷  ");
    assert_eq!(nav_symbol(Nav::Up(1)).format(), "  △  ");
    assert_eq!(nav_symbol(Nav::Up(2)).format(), "  △² ");
    assert_eq!(nav_symbol(Nav::UpSkipTrivia(1)).format(), " !△  ");
    assert_eq!(nav_symbol(Nav::UpExact(1)).format(), "!!△  ");
}

#[test]
fn test_width_for_count() {
    assert_eq!(width_for_count(0), 1);
    assert_eq!(width_for_count(1), 1);
    assert_eq!(width_for_count(10), 1);
    assert_eq!(width_for_count(11), 2);
    assert_eq!(width_for_count(100), 2);
    assert_eq!(width_for_count(101), 3);
}

#[test]
fn test_truncate_text() {
    assert_eq!(truncate_text("hello", 10), "hello");
    assert_eq!(truncate_text("hello world", 10), "hello wor…");
    assert_eq!(truncate_text("abc", 3), "abc");
    assert_eq!(truncate_text("abcd", 3), "ab…");
}

#[test]
fn test_superscript() {
    assert_eq!(superscript(0), "⁰");
    assert_eq!(superscript(1), "¹");
    assert_eq!(superscript(9), "⁹");
    assert_eq!(superscript(10), "¹⁰");
    assert_eq!(superscript(12), "¹²");
}
