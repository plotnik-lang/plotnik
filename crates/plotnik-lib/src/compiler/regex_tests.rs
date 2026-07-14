use regex_automata::Input;
use regex_automata::dfa::{Automaton, dense};
use regex_syntax::hir::{Class, ClassUnicode, HirKind, Look};

use super::regex::print::ecma;
use super::regex::{compile_native_dfa, is_normalized, normalize};

#[test]
fn case_insensitive_normalization_pins_unicode_folding() {
    let normalized = normalize("(?i)k");

    let HirKind::Class(Class::Unicode(class)) = normalized.kind() else {
        panic!("case-folded literal must normalize to a Unicode class");
    };
    assert!(class_contains(class, 'K'));
    assert!(class_contains(class, 'k'));
    assert!(class_contains(class, '\u{212A}'));
}

#[test]
fn unicode_word_boundaries_normalize_to_ascii() {
    assert!(matches!(
        normalize(r"\b").kind(),
        HirKind::Look(Look::WordAscii)
    ));
    assert!(matches!(
        normalize(r"\B").kind(),
        HirKind::Look(Look::WordAsciiNegate)
    ));
}

#[test]
fn dot_normalizes_to_explicit_scalar_ranges() {
    let normalized = normalize(".");

    let HirKind::Class(Class::Unicode(class)) = normalized.kind() else {
        panic!("Unicode dot must normalize to a Unicode class");
    };
    assert!(class_contains(class, '\r'));
    assert!(!class_contains(class, '\n'));
    assert!(class_contains(class, '\u{2028}'));
}

#[test]
fn normalized_hir_contains_only_the_portable_contract() {
    for pattern in [
        "abc",
        "^a|b$",
        "(?i)k",
        "(?s:.)",
        r"\d\w\s\p{Lu}",
        "[a-z&&[^q]]",
        "(capture)(?:group)",
        "(?:ab)+?",
        r"\bword\B",
        r"\P{any}",
    ] {
        assert!(is_normalized(&normalize(pattern)), "/{pattern}/");
    }
}

#[test]
fn normalization_preserves_search_except_for_deliberate_boundaries() {
    let patterns = [
        "",
        "^$",
        "^x",
        "x$",
        "a|bc",
        "x+",
        "a.*?b",
        "[0-9]{4}",
        "π+",
        "^π$",
        r"(?:π|x)\w*",
        "(?s:.)*",
        "(?i)k",
        r"\p{Lu}",
        "[a-z&&[^q]]",
        "(capture)",
    ];
    let texts = [
        "",
        "x",
        "ax",
        "xa",
        "xx",
        "a",
        "bc",
        "zbcq",
        "a___b___b",
        "123 foobar 4567",
        "foo",
        "π",
        "aπb",
        "ππ",
        "πx_42",
        "🦀 x",
        "x\ny",
        "y\nx\nz",
        "K",
        "\u{212A}",
        "capture",
    ];

    for pattern in patterns {
        let raw = build_raw(pattern);
        let bytes = compile_native_dfa(&normalize(pattern)).expect("normalized regex compiles");
        let normalized = plotnik_rt::deserialize_dfa(&bytes).expect("normalized DFA loads");

        for text in texts {
            let expected = raw
                .try_search_fwd(&Input::new(text))
                .expect("raw search completes")
                .is_some();
            let actual = normalized
                .try_search_fwd(&Input::new(text))
                .expect("normalized search completes")
                .is_some();
            assert_eq!(actual, expected, "/{pattern}/ against {text:?}");
        }
    }

    assert!(!raw_is_match(r"\bx", "πx"));
    assert!(normalized_is_match(r"\bx", "πx"));
}

#[test]
fn ecma_prints_astral_ranges_and_unicode_flag() {
    let printed = ecma::print(&normalize(r"[\u{1F980}-\u{1F99F}]"));

    assert_eq!(printed.source, r"[\u{1f980}-\u{1f99f}]");
    assert_eq!(printed.flags, "u");
}

#[test]
fn ecma_prints_folded_classes_without_ignore_case() {
    let printed = ecma::print(&normalize("(?i)k"));

    assert_eq!(printed.source, r"[Kk\u{212a}]");
    assert_eq!(printed.flags, "u");
}

#[test]
fn ecma_prints_anchors_and_empty_alternative() {
    assert_eq!(ecma::print(&normalize("^$")).source, "^$");
    assert_eq!(ecma::print(&normalize("a|")).source, "a|(?:)");
}

#[test]
fn ecma_prints_explicit_always_and_never_match_classes() {
    assert_eq!(
        ecma::print(&normalize(r"\p{any}")).source,
        r"[\u{0}-\u{d7ff}\u{e000}-\u{10ffff}]"
    );
    assert_eq!(ecma::print(&normalize(r"\P{any}")).source, "[]");
}

#[test]
fn ecma_groups_multi_scalar_repetition_operands() {
    assert_eq!(ecma::print(&normalize("(ab)+")).source, "(?:ab)+");
}

#[test]
fn ecma_groups_repeated_assertions_and_keeps_plain_hyphens() {
    assert_eq!(ecma::print(&normalize("(?:^)*")).source, "(?:^)?");
    assert_eq!(ecma::print(&normalize("-")).source, "-");
}

fn class_contains(class: &ClassUnicode, character: char) -> bool {
    class
        .ranges()
        .iter()
        .any(|range| range.start() <= character && character <= range.end())
}

fn build_raw(pattern: &str) -> dense::DFA<Vec<u32>> {
    dense::DFA::builder()
        .configure(
            dense::DFA::config()
                .start_kind(regex_automata::dfa::StartKind::Unanchored)
                .minimize(true),
        )
        .build(pattern)
        .expect("test regex compiles")
}

fn raw_is_match(pattern: &str, text: &str) -> bool {
    regex_automata::meta::Regex::new(pattern)
        .expect("test regex compiles")
        .is_match(text)
}

fn normalized_is_match(pattern: &str, text: &str) -> bool {
    let bytes = compile_native_dfa(&normalize(pattern)).expect("normalized regex compiles");
    let dfa = plotnik_rt::deserialize_dfa(&bytes).expect("normalized DFA loads");
    dfa.try_search_fwd(&Input::new(text))
        .expect("normalized search completes")
        .is_some()
}
