//! End-to-end tests of `query!` through the `plotnik` facade: grammar
//! resolution against real Arborium and Tree-sitter dependency-graph packages,
//! the file form, compiled-in limits, and the version-skew
//! assert.

use plotnik::LimitExceeded;
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

mod tree_sitter_package {
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
        fuel = 1,
    }
}

// Recursive output nests `Expr` values through native decoder calls, so a depth
// policy of 1 must trip `parse`; `matches` suppresses output and never decodes.
mod depth_limited {
    plotnik::query! {
        r#"
        Expr = [
          Id: (identifier) @id
          Paren: (parenthesized_expression (Expr) @expr)
        ]
        Q = (program (expression_statement (Expr) @expr))
        "#,
        grammar = "arborium-javascript",
        depth = 1,
    }
}

mod scalar_output {
    plotnik::query! {
        r#"
        Q = (program
          (comment)* @comments :: text
          (expression_statement)? @has_statement :: bool
        )
        "#,
        grammar = "arborium-javascript",
    }
}

mod mixed_borrows {
    plotnik::query! {
        "Q = (program (expression_statement (identifier) @text :: text) @statement)",
        grammar = "arborium-javascript",
    }
}

mod recursive_split_returns {
    plotnik::query! {
        r#"
        Body = [
          Rec: {(comment) (B)}
          Base: (comment)
        ]
        B = {(Body)?? (Body)?}
        Q = (program {
          (B)
          .!
          [(comment == "//b") (comment == "//stop")] @rest :: text
        })
        "#,
        grammar = "arborium-javascript",
    }
}

mod recursive_routed_returns {
    plotnik::query! {
        r#"
        A = [
          (statement_block (A)+ (debugger_statement))
          (statement_block (debugger_statement))
        ]?
        Q = (program (A) (expression_statement) @e)
        "#,
        grammar = "arborium-javascript",
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

    let value = queries::Idents::parse(&tree, source)
        .expect("auto limits fit")
        .expect("matches");

    assert_eq!(value.id.utf8_text(source.as_bytes()), Ok("x"));
    assert!(queries::Idents::matches(&tree, source).expect("auto limits fit"));
}

#[test]
fn generic_surface_delegates_to_generated_types() {
    let source = "x;";
    let tree = parse(&js(), source);

    let value = plotnik::parse::<queries::Idents>(&tree, source)
        .expect("auto limits fit")
        .expect("matches");

    assert_eq!(value.id.utf8_text(source.as_bytes()), Ok("x"));
    assert!(plotnik::matches::<queries::Idents>(&tree, source).expect("auto limits fit"));
    assert!(plotnik::matches::<queries::Probe>(&tree, source).expect("auto limits fit"));
}

#[test]
fn void_definition_exposes_matches() {
    let source = "x;";
    let tree = parse(&js(), source);

    assert!(queries::Probe::matches(&tree, source).expect("auto limits fit"));
}

#[test]
fn tree_sitter_grammar_resolves() {
    let source = "x;";
    let tree = parse(&tree_sitter_javascript::LANGUAGE.into(), source);

    let value = tree_sitter_package::Q::parse(&tree, source)
        .expect("auto limits fit")
        .expect("matches");

    assert_eq!(value.id.utf8_text(source.as_bytes()), Ok("x"));
}

#[test]
fn subgrammar_selection_resolves_tsx() {
    let source = "x;";
    let tree = parse(&tree_sitter_typescript::LANGUAGE_TSX.into(), source);

    let value = tsx::Q::parse(&tree, source)
        .expect("auto limits fit")
        .expect("matches");

    assert_eq!(value.id.utf8_text(source.as_bytes()), Ok("x"));
}

#[test]
fn file_form_reads_next_to_the_invoking_file() {
    let source = "x;";
    let tree = parse(&js(), source);

    let value = from_file::Q::parse(&tree, source)
        .expect("auto limits fit")
        .expect("matches");

    assert_eq!(value.id.utf8_text(source.as_bytes()), Ok("x"));
}

#[test]
fn compiled_in_fuel_limit_trips_safe_surfaces() {
    let source = "x;";
    let tree = parse(&js(), source);

    assert!(matches!(
        limited::Q::parse(&tree, source),
        Err(LimitExceeded::OutOfFuel(1))
    ));
    assert!(matches!(
        limited::Q::matches(&tree, source),
        Err(LimitExceeded::OutOfFuel(1))
    ));
}

#[test]
fn compiled_in_depth_limit_trips_parse_only() {
    let source = "((x));";
    let tree = parse(&js(), source);

    assert!(matches!(
        depth_limited::Q::parse(&tree, source),
        Err(LimitExceeded::DecodeDepth(1))
    ));
    assert!(depth_limited::Q::matches(&tree, source).expect("matches ignores decode depth"));
}

#[test]
fn serde_feature_flows_through_the_facade() {
    let source = "x;";
    let tree = parse(&js(), source);
    let value = queries::Idents::parse(&tree, source)
        .expect("auto limits fit")
        .expect("matches");

    let json =
        serde_json::to_string(&plotnik::rt::WithSource::new(&value, source)).expect("serializes");

    assert!(json.contains("\"id\""), "got: {json}");
}

#[test]
fn generated_scalars_preserve_items_and_presence() {
    let source = "// first\n\n// second\nx;";
    let tree = parse(&js(), source);

    let value = scalar_output::Q::parse(&tree, source)
        .expect("auto limits fit")
        .expect("matches");

    assert_eq!(value.comments, ["// first", "// second"]);
    assert!(value.has_statement);
}

#[test]
fn generated_bool_uses_absence_not_truthiness() {
    let source = "// only";
    let tree = parse(&js(), source);

    let value = plotnik::parse::<scalar_output::Q>(&tree, source)
        .expect("auto limits fit")
        .expect("matches");

    assert_eq!(value.comments, ["// only"]);
    assert!(!value.has_statement);
}

#[test]
fn generated_output_can_borrow_tree_and_source_independently() {
    let source = "name;";
    let tree = parse(&js(), source);

    let value = mixed_borrows::Q::parse(&tree, source)
        .expect("auto limits fit")
        .expect("matches");

    assert_eq!(value.text, "name");
    assert_eq!(value.statement.utf8_text(source.as_bytes()), Ok("name;"));
}

#[test]
fn generated_matcher_preserves_mixed_recursive_greediness() {
    let source = "//a\n//b\n//stop";
    let tree = parse(&js(), source);

    let value = recursive_split_returns::Q::parse(&tree, source)
        .expect("auto limits fit")
        .expect("matches");

    assert_eq!(value.rest, "//stop");
}

#[test]
fn generated_matcher_executes_routed_required_recursive_call() {
    let source = "{ { debugger; } debugger; } f();";
    let tree = parse(&js(), source);

    let value = recursive_routed_returns::Q::parse(&tree, source)
        .expect("auto limits fit")
        .expect("matches");

    assert_eq!(
        value
            .e
            .utf8_text(source.as_bytes())
            .expect("captured node lies within UTF-8 source"),
        "f();"
    );
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
