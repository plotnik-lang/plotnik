use regex_automata::Input;
use regex_automata::dfa::Automaton;

use super::regex::{compile_native_dfa, compile_portable_dfa};

#[test]
fn portable_dfa_matches_native_search() {
    let patterns = [
        "",
        "^$",
        "^x",
        "x$",
        "a|bc",
        "x+",
        "a.*?b",
        "[0-9]{4}",
        r"(?-u:\b)foo(?-u:\b)",
        "π+",
        "^π$",
        r"(?:π|x)\w*",
        "(?m:^x$)",
        "(?s:.)*",
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
        "seafood fool",
        "π",
        "aπb",
        "ππ",
        "πx_42",
        "🦀 x",
        "x\ny",
        "y\nx\nz",
    ];

    for pattern in patterns {
        let bytes = compile_native_dfa(pattern).expect("test regex compiles");
        let native = plotnik_rt::deserialize_dfa(&bytes).expect("native DFA loads");
        let portable = compile_portable_dfa(pattern).expect("portable DFA compiles");

        for text in texts {
            let expected = native
                .try_search_fwd(&Input::new(text))
                .expect("native search completes")
                .is_some();
            let actual = portable
                .is_match(text.as_bytes())
                .expect("portable search completes");
            assert_eq!(actual, expected, "/{pattern}/ against {text:?}");
        }
    }
}

#[test]
fn native_and_portable_builds_reject_the_same_invalid_pattern() {
    let pattern = "[";

    let native = compile_native_dfa(pattern);
    let portable = compile_portable_dfa(pattern);

    assert!(native.is_err());
    assert!(portable.is_err());
}
