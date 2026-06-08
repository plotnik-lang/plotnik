use plotnik_core::grammar::raw::{RawGrammar, RawRule};

use super::lang::GrammarRenderer;
use plotnik::language_registry::{self, Lang};

fn smoke_test(lang: &Lang, source: &str, expected_root: &str) {
    let tree = lang.parse(source);
    let root = tree.root_node();
    assert_eq!(root.kind(), expected_root);
    assert!(!root.has_error());
}

#[test]
#[cfg(feature = "lang-javascript")]
fn smoke_parse_javascript() {
    smoke_test(
        language_registry::javascript(),
        "function hello() { return 42; }",
        "program",
    );
}

#[test]
fn grammar_dump_renders_synthetic_raw_grammar() {
    let grammar = RawGrammar {
        name: "test".to_string(),
        rules: [
            (
                "program".to_string(),
                RawRule::SEQ {
                    members: vec![RawRule::FIELD {
                        name: "body".to_string(),
                        content: Box::new(RawRule::CHOICE {
                            members: vec![
                                RawRule::SYMBOL {
                                    name: "statement".to_string(),
                                },
                                RawRule::BLANK,
                            ],
                        }),
                    }],
                },
            ),
            (
                "statement".to_string(),
                RawRule::STRING {
                    value: "let".to_string(),
                },
            ),
        ]
        .into_iter()
        .collect(),
        extras: vec![RawRule::SYMBOL {
            name: "comment".to_string(),
        }],
        precedences: Vec::new(),
        conflicts: Vec::new(),
        externals: Vec::new(),
        inline: Vec::new(),
        supertypes: Vec::new(),
        word: None,
        reserved: Default::default(),
        inherits: None,
    };

    let output = GrammarRenderer::new(&grammar).render();

    assert!(output.contains("extras = [\n  (comment)\n]\n\n"));
    assert!(output.contains("program = {\n  body: (statement)?\n}\n\n"));
    assert!(output.contains("statement = \"let\"\n\n"));
}

#[test]
fn lang_info_has_aliases() {
    let langs = language_registry::all();
    assert!(!langs.is_empty());

    for lang in &langs {
        assert!(!lang.name().is_empty(), "name should not be empty");
        assert!(
            !lang.aliases().is_empty(),
            "aliases should not be empty for {}",
            lang.name()
        );
    }
}

#[test]
fn lang_from_name_canonical() {
    let langs = language_registry::all();

    for lang in &langs {
        let resolved = language_registry::from_name(lang.name());
        assert!(
            resolved.is_some(),
            "canonical name '{}' should resolve",
            lang.name()
        );
    }
}

#[test]
fn lang_from_name_aliases() {
    let langs = language_registry::all();

    for lang in &langs {
        for alias in lang.aliases() {
            let resolved = language_registry::from_name(alias);
            assert!(resolved.is_some(), "alias '{}' should resolve", alias);
        }
    }
}

#[test]
fn lang_extensions_are_exposed() {
    let langs = language_registry::all();

    for lang in &langs {
        assert!(
            !lang.extensions().is_empty(),
            "extensions should not be empty for {}",
            lang.name()
        );
    }
}
