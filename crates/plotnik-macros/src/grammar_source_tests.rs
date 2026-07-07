use std::path::Path;

use crate::grammar_source::{GrammarSpec, parse_spec, resolve, resolve_relative};

#[test]
fn spec_json_suffix_is_a_path() {
    assert_eq!(
        parse_spec("./grammars/mylang.json"),
        GrammarSpec::Path("./grammars/mylang.json")
    );
}

#[test]
fn spec_bare_name_is_a_package() {
    assert_eq!(
        parse_spec("tree-sitter-javascript"),
        GrammarSpec::Package {
            name: "tree-sitter-javascript",
            subgrammar: None
        }
    );
}

#[test]
fn spec_slash_selects_a_subgrammar() {
    assert_eq!(
        parse_spec("tree-sitter-typescript/tsx"),
        GrammarSpec::Package {
            name: "tree-sitter-typescript",
            subgrammar: Some("tsx")
        }
    );
}

#[test]
fn relative_path_joins_the_base() {
    let resolved = resolve_relative("queries/q.ptk", Some(Path::new("/base"))).expect("resolves");

    assert_eq!(resolved, Path::new("/base/queries/q.ptk"));
}

#[test]
fn relative_path_without_a_base_is_an_error() {
    let error = resolve_relative("queries/q.ptk", None).expect_err("no base to join");

    assert!(error.contains("use an absolute path"), "got: {error}");
}

#[test]
fn unknown_package_reports_the_dependency_graph() {
    // The test process runs under cargo, so `cargo metadata` resolves this
    // crate's real graph — which certainly has no such package.
    let spec = GrammarSpec::Package {
        name: "plotnik-no-such-grammar-crate",
        subgrammar: None,
    };

    let error = resolve(&spec, None).expect_err("package cannot exist");

    assert!(
        error.contains("not in this crate's dependency graph"),
        "got: {error}"
    );
}

#[test]
fn package_without_grammar_json_says_so() {
    // In the graph (it is our own dependency), but ships no grammar.json.
    let spec = GrammarSpec::Package {
        name: "plotnik-lib",
        subgrammar: None,
    };

    let error = resolve(&spec, None).expect_err("plotnik-lib ships no grammar");

    assert!(error.contains("ships no grammar.json"), "got: {error}");
}
