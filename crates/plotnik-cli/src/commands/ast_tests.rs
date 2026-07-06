//! The source-AST dump doubles as copy-pasteable Plotnik patterns: leaf text
//! renders as a `==` predicate, keyword leaves collapse to their kind, and
//! containers/ERROR stay bare (see `plotnik_lib::dump_tree_text`).

#![cfg(feature = "lang-javascript")]

use indoc::indoc;
use plotnik_lib::dump_tree_text;

use crate::language_registry;

fn dump_js(source: &str, raw: bool) -> String {
    let lang = language_registry::from_name("javascript").expect("javascript is enabled");
    let tree = lang.parse_source(source);
    dump_tree_text(&tree, source, lang.grammar(), raw)
}

#[test]
fn leaf_text_renders_as_predicate() {
    let dump = dump_js("const version = 1;\n", false);

    assert_eq!(
        dump,
        indoc! {r#"
            (program
              (lexical_declaration
                (variable_declarator
                  name: (identifier == "version")
                  value: (number == "1"))))
        "#}
    );
}

#[test]
fn keyword_leaf_collapses_to_kind() {
    let dump = dump_js("this;\n", false);

    assert_eq!(
        dump,
        indoc! {"
            (program
              (expression_statement
                (this)))
        "}
    );
}

#[test]
fn empty_container_stays_bare() {
    let dump = dump_js("function f() {}\n", false);

    assert_eq!(
        dump,
        indoc! {r#"
            (program
              (function_declaration
                name: (identifier == "f")
                parameters: (formal_parameters)
                body: (statement_block)))
        "#}
    );
}

#[test]
fn escapes_match_query_string_syntax() {
    let dump = dump_js("// back\\slash\n", false);

    assert_eq!(
        dump,
        indoc! {r#"
            (program
              (comment == "// back\\slash"))
        "#}
    );
}

#[test]
fn error_leaf_keeps_text_in_comment() {
    let dump = dump_js("function (", false);

    assert_eq!(
        dump,
        indoc! {r#"
            (program
              (ERROR) ; "function (")
        "#}
    );
}

#[test]
fn raw_mode_renders_anonymous_tokens_as_strings() {
    let dump = dump_js("1;\n", true);

    assert_eq!(
        dump,
        indoc! {r#"
            (program
              (expression_statement
                (number == "1")
                ";"))
        "#}
    );
}
