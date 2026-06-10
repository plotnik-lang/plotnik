//! Test utilities and snapshot macros.

use plotnik_core::grammar::{Grammar, raw::RawGrammar};

pub fn colliding_node_type_grammar() -> Grammar {
    let raw = RawGrammar::from_json(
        r#"{
        "name": "collision",
        "rules": {
            "program": {
                "type": "CHOICE",
                "members": [
                    { "type": "SYMBOL", "name": "number" },
                    { "type": "STRING", "value": "number" }
                ]
            },
            "number": { "type": "STRING", "value": "literal" }
        }
    }"#,
    )
    .expect("collision grammar fixture");

    Grammar::from_raw(&raw).expect("collision grammar metadata")
}

/// Snapshot test for bytecode output.
#[macro_export]
macro_rules! shot_bytecode {
    ($query:literal) => {{
        let query = indoc::indoc!($query).trim();
        let output = $crate::Query::expect_valid_bytecode(query);
        insta::with_settings!({ omit_expression => true }, {
            insta::assert_snapshot!(format!("{query}\n---\n{output}"));
        });
    }};
}

/// Snapshot test for CST output.
#[macro_export]
macro_rules! shot_cst {
    ($query:literal) => {{
        let query = indoc::indoc!($query).trim();
        let output = $crate::Query::expect_valid_cst(query);
        insta::with_settings!({ omit_expression => true }, {
            insta::assert_snapshot!(format!("{query}\n---\n{output}"));
        });
    }};
}

/// Snapshot test for AST output.
#[macro_export]
macro_rules! shot_ast {
    ($query:literal) => {{
        let query = indoc::indoc!($query).trim();
        let output = $crate::Query::expect_valid_ast(query);
        insta::with_settings!({ omit_expression => true }, {
            insta::assert_snapshot!(format!("{query}\n---\n{output}"));
        });
    }};
}

/// Snapshot test for TypeScript type output.
#[macro_export]
macro_rules! shot_types {
    ($query:literal) => {{
        let query = indoc::indoc!($query).trim();
        let output = $crate::Query::expect_valid_types(query);
        insta::with_settings!({ omit_expression => true }, {
            insta::assert_snapshot!(format!("{query}\n---\n{output}"));
        });
    }};
}

/// Snapshot test for error diagnostics.
#[macro_export]
macro_rules! shot_error {
    ($query:literal) => {{
        let query = indoc::indoc!($query).trim();
        let output = $crate::Query::expect_invalid(query);
        insta::with_settings!({ omit_expression => true }, {
            insta::assert_snapshot!(format!("{query}\n---\n{output}"));
        });
    }};
}
