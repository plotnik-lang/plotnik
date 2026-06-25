use plotnik_lib::grammar::DumpOptions;

use crate::language_registry::{self, Lang};

fn smoke_test(lang: &Lang, source: &str, expected_root: &str) {
    let tree = lang.parse_source(source);
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

/// The canonical end-to-end document: every register (pattern/type/text), the
/// extras line, hidden splices, the category, and the `; root`/`; extra` notes.
#[test]
#[cfg(feature = "lang-json")]
fn dump_json_document() {
    let dump = language_registry::json()
        .grammar()
        .tree()
        .dump(&DumpOptions::default());
    insta::assert_snapshot!("dump_json", dump);
}

#[test]
#[cfg(feature = "lang-json")]
fn dump_no_legend_strips_header() {
    let lang = language_registry::json();
    let with = lang.grammar().tree().dump(&DumpOptions {
        legend: true,
        ..DumpOptions::default()
    });
    let without = lang.grammar().tree().dump(&DumpOptions {
        legend: false,
        ..DumpOptions::default()
    });
    assert!(with.starts_with("; json — how its trees are shaped"));
    assert!(!without.contains("; json — how its trees are shaped"));
    assert!(without.starts_with("extras = "));
}

/// A supertype renders as a de-underscored `name#` category, and a nested
/// supertype member stays compressed as one `member#` entry.
#[test]
#[cfg(feature = "lang-rust")]
fn dump_rust_category_closure() {
    let dump = language_registry::rust()
        .grammar()
        .tree()
        .dump(&DumpOptions {
            legend: false,
            ..DumpOptions::default()
        });
    assert!(dump.lines().any(|line| line == "expression# ="));
    assert!(dump.contains("\n  | literal#\n"));
}

/// A non-underscore rule placed in the grammar's `inline` list is hidden, and
/// carries the `; inlined` annotation.
#[test]
#[cfg(feature = "lang-devicetree")]
fn dump_devicetree_inlined_annotation() {
    let dump = language_registry::devicetree()
        .grammar()
        .tree()
        .dump(&DumpOptions {
            legend: false,
            ..DumpOptions::default()
        });
    assert!(dump.contains("; inlined"));
}

/// The `#` namespace lets a category and a concrete node share a base name.
#[test]
#[cfg(feature = "lang-ocaml")]
fn dump_ocaml_category_node_coexistence() {
    let dump = language_registry::ocaml()
        .grammar()
        .tree()
        .dump(&DumpOptions {
            legend: false,
            ..DumpOptions::default()
        });
    assert!(dump.lines().any(|line| line.starts_with("parameter# =")));
    assert!(dump.lines().any(|line| line.starts_with("parameter = ")));
}

/// A group short enough to fit the baseline width folds onto one line.
#[test]
#[cfg(feature = "lang-json")]
fn dump_folds_short_groups_inline() {
    let dump = language_registry::json()
        .grammar()
        .tree()
        .dump(&DumpOptions {
            legend: false,
            ..DumpOptions::default()
        });
    assert!(
        dump.lines()
            .any(|line| line == r#"object = { "{" { (pair) { "," (pair) }* }? "}" }"#)
    );
}

/// `width: 0` restores always-break: no group fits, so every composite spans
/// multiple lines.
#[test]
#[cfg(feature = "lang-json")]
fn dump_width_zero_always_breaks() {
    let dump = language_registry::json().grammar().tree().dump(&DumpOptions {
        legend: false,
        width: 0,
    });
    // The object body opens but its children are pushed onto their own lines.
    assert!(dump.contains("object = {\n"));
    assert!(!dump.contains(r#"object = { "{""#));
}
