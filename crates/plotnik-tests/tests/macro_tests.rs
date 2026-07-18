//! Compile-time boundaries that the snapshot corpus cannot exercise: proc-macro
//! grammar resolution, facade wiring, baked-in limits, and grammar skew.

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

mod queries {
    plotnik::query! {
        "Idents = (program (expression_statement (identifier) @id))",
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

mod skew {
    plotnik::query! {
        "Q = (program (expression_statement (identifier) @id))",
        grammar = "arborium-javascript",
    }
}

#[test]
fn facade_executes_generated_types_from_multiple_expansions() {
    let source = "x;";
    let tree = parse(&js(), source);

    let value = plotnik::parse::<queries::Idents>(&tree, source)
        .expect("auto limits fit")
        .expect("matches");
    let json =
        serde_json::to_string(&plotnik::rt::WithSource::new(&value, source)).expect("serializes");

    assert_eq!(value.id.utf8_text(source.as_bytes()), Ok("x"));
    assert!(plotnik::matches::<queries::Idents>(&tree, source).expect("auto limits fit"));
    assert!(plotnik::matches::<queries::Probe>(&tree, source).expect("auto limits fit"));
    assert!(json.contains("\"id\""), "got: {json}");
}

#[test]
fn grammar_packages_and_file_form_resolve() {
    let source = "x;";
    let arborium_tree = parse(&js(), source);
    let javascript_tree = parse(&tree_sitter_javascript::LANGUAGE.into(), source);
    let tsx_tree = parse(&tree_sitter_typescript::LANGUAGE_TSX.into(), source);

    assert!(from_file::Q::matches(&arborium_tree, source).expect("auto limits fit"));
    assert!(tree_sitter_package::Q::matches(&javascript_tree, source).expect("auto limits fit"));
    assert!(tsx::Q::matches(&tsx_tree, source).expect("auto limits fit"));
}

#[test]
fn compiled_limits_reject_work_at_their_distinct_boundaries() {
    let source = "((x));";
    let tree = parse(&js(), source);

    assert!(matches!(
        limited::Q::matches(&tree, source),
        Err(LimitExceeded::OutOfFuel(1))
    ));
    assert!(matches!(
        depth_limited::Q::parse(&tree, source),
        Err(LimitExceeded::DecodeDepth(1))
    ));
    assert!(depth_limited::Q::matches(&tree, source).expect("matches skips decoding"));
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
