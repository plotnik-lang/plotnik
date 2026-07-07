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
fn node_table_maps_dump_ranges_to_source_ranges() {
    let lang = language_registry::from_name("javascript").expect("javascript is enabled");
    let source = "const version = 1;\n";
    let tree = lang.parse_source(source);

    let dump = plotnik_lib::dump_tree(&tree, source, lang.grammar(), false);
    let text: String = dump
        .chunks
        .iter()
        .map(|chunk| chunk.text.as_str())
        .collect();

    // program, lexical_declaration, variable_declarator, identifier, number.
    assert_eq!(dump.nodes.len(), 5);
    let root = dump.nodes[0];
    assert_eq!(root.dump_start, 0);
    assert_eq!(&text[root.dump_start..root.dump_end], text.trim_end());
    let ident = dump.nodes[3];
    assert_eq!(
        &text[ident.dump_start..ident.dump_end],
        r#"name: (identifier == "version")"#
    );
    assert_eq!(&source[ident.src_start..ident.src_end], "version");
    // Pre-order, nested inside the root: the playground's hover targeting
    // (innermost node containing an offset) relies on both.
    for pair in dump.nodes.windows(2) {
        assert!(pair[0].dump_start < pair[1].dump_start);
    }
    for node in &dump.nodes[1..] {
        assert!(node.dump_start >= root.dump_start && node.dump_end <= root.dump_end);
    }
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
