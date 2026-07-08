//! End-to-end tests of `query!` through the `plotnik` facade: grammar
//! resolution against real dependency-graph packages (arborium and vanilla
//! tree-sitter crates), the file form, compiled-in limits, the `crate`
//! override, and the version-skew assert.

use plotnik::rt::LimitError;
use plotnik::tree_sitter::{Language, Parser, Tree};

fn parse(language: &Language, source: &str) -> Tree {
    let mut parser = Parser::new();
    parser.set_language(language).expect("language loads");
    parser.parse(source, None).expect("source parses")
}

fn js() -> Language {
    arborium_javascript::language().into()
}

// Two invocations sharing one module: each expansion lives in its own
// fingerprint-named wrapper, so their internals (the `rt` alias,
// `mod matcher`) never collide.
mod queries {
    plotnik::query! {
        r#"
        Idents = (program (expression_statement (identifier) @id))
        "#,
        grammar = "arborium-javascript",
    }

    plotnik::query! {
        "Probe = {(program)}",
        grammar = "arborium-javascript",
    }
}

mod vanilla {
    plotnik::query! {
        "Q = (program (expression_statement (identifier) @id))",
        grammar = "tree-sitter-javascript",
    }
}

mod tsx {
    plotnik::query! {
        "Q = (program (expression_statement (identifier) @id))",
        grammar = "tree-sitter-typescript/tsx",
    }
}

mod from_file {
    plotnik::query! {
        grammar = "arborium-javascript",
        file = "macro_queries/idents.ptk",
    }
}

mod limited {
    plotnik::query! {
        "Q = (program (expression_statement (identifier) @id))",
        grammar = "arborium-javascript",
        steps = 1,
    }
}

// Rows nest the committed value three scopes deep (struct, array, row
// struct), so a depth policy of 1 must trip the metered path.
mod depth_limited {
    plotnik::query! {
        "Q = (program (expression_statement (identifier) @id)* @rows)",
        grammar = "arborium-javascript",
        depth = 1,
    }
}

mod repointed {
    plotnik::query! {
        "Q = (program (expression_statement (identifier) @id))",
        grammar = "arborium-javascript",
        crate = ::plotnik::rt,
    }
}

// Dedicated module for the skew test: its first (and only) run must be the
// wrong-language one, and the language check is once-per-module.
mod skew {
    plotnik::query! {
        "Q = (program (expression_statement (identifier) @id))",
        grammar = "arborium-javascript",
    }
}

#[test]
fn arborium_grammar_produces_typed_output() {
    let source = "x;";
    let tree = parse(&js(), source);

    let value = queries::Idents::parse(&tree, source).expect("matches");

    assert_eq!(value.id.utf8_text(source.as_bytes()), Ok("x"));
}

#[test]
fn void_definition_exposes_matches() {
    let source = "x;";
    let tree = parse(&js(), source);

    assert!(queries::Probe::matches(&tree, source).expect("auto limits fit"));
}

#[test]
fn vanilla_tree_sitter_grammar_resolves() {
    let source = "x;";
    let tree = parse(&tree_sitter_javascript::LANGUAGE.into(), source);

    let value = vanilla::Q::parse(&tree, source).expect("matches");

    assert_eq!(value.id.utf8_text(source.as_bytes()), Ok("x"));
}

#[test]
fn subgrammar_selection_resolves_tsx() {
    let source = "x;";
    let tree = parse(&tree_sitter_typescript::LANGUAGE_TSX.into(), source);

    let value = tsx::Q::parse(&tree, source).expect("matches");

    assert_eq!(value.id.utf8_text(source.as_bytes()), Ok("x"));
}

#[test]
fn file_form_reads_next_to_the_invoking_file() {
    let source = "x;";
    let tree = parse(&js(), source);

    let value = from_file::Q::parse(&tree, source).expect("matches");

    assert_eq!(value.id.utf8_text(source.as_bytes()), Ok("x"));
}

#[test]
fn compiled_in_step_limit_trips_try_parse_only() {
    let source = "x;";
    let tree = parse(&js(), source);

    assert!(limited::Q::parse(&tree, source).is_some());
    assert!(matches!(
        limited::Q::try_parse(&tree, source),
        Err(LimitError::Steps(1))
    ));
}

#[test]
fn compiled_in_depth_limit_trips_try_parse_only() {
    let source = "x;";
    let tree = parse(&js(), source);

    assert!(depth_limited::Q::parse(&tree, source).is_some());
    assert!(matches!(
        depth_limited::Q::try_parse(&tree, source),
        Err(LimitError::Depth(1))
    ));
}

#[test]
fn crate_override_respells_the_runtime_path() {
    let source = "x;";
    let tree = parse(&js(), source);

    assert!(repointed::Q::parse(&tree, source).is_some());
}

#[test]
fn serde_feature_flows_through_the_facade() {
    let source = "x;";
    let tree = parse(&js(), source);
    let value = queries::Idents::parse(&tree, source).expect("matches");

    let json =
        serde_json::to_string(&plotnik::rt::WithSource::new(&value, source)).expect("serializes");

    assert!(json.contains("\"id\""), "got: {json}");
}

#[test]
fn wrong_language_tree_panics_with_version_skew() {
    let source = "void main() {}";
    let tree = parse(&arborium_dart::language().into(), source);

    let panic = std::panic::catch_unwind(|| skew::Q::parse(&tree, source))
        .expect_err("the language check must reject a dart tree");

    let message = panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied())
        .expect("panic carries a message");
    assert!(message.contains("grammar version skew"), "got: {message}");
}
