//! `check` runs the full compile pipeline as a dry run: queries that pass
//! analysis but fail compile/emit/load must be rejected with exit 1 (`CliError::No`),
//! never panic and never silently succeed.

#![cfg(feature = "lang-javascript")]

use super::check::{self, CheckArgs};
use crate::error::{CliError, CliResult};

fn check_js(query: &str) -> CliResult {
    check::run(CheckArgs {
        query_path: None,
        query_text: Some(query.to_string()),
        lang: Some("javascript".to_string()),
        strict: false,
        json: false,
        color: false,
    })
}

#[test]
fn accepts_labeled_empty_alternative_in_quantifier() {
    // A skippable labeled alternative in a quantifier (`[A: (comment)? @c]*`) used to emit
    // an unbracketed skip path rejected by bytecode validation;
    // variant bracket dominance makes both paths close the variant, so it compiles.
    assert!(check_js("Q = (program [A: (comment)? @c]* @items)").is_ok());
}

#[test]
fn rejects_byte_oriented_regex_predicate() {
    // Passes analysis; the DFA build fails at emit time (EmitError::RegexCompile).
    assert!(matches!(
        check_js(r"Q = (identifier =~ /(?-u:\xFF)/) @x"),
        Err(CliError::No)
    ));
}

#[test]
fn rejects_definition_with_positional_only_body() {
    // `.` constrains a position inside a node; it is not a definition pattern by itself.
    assert!(matches!(check_js("Q = ."), Err(CliError::No)));
}

#[test]
fn rejects_positional_only_definition_among_valid_definitions() {
    // A positional-only definition never reaches analysis, even when another
    // definition compiles; `check` must still flag it instead of exiting 0.
    assert!(matches!(
        check_js("Bad = .\nGood = (identifier) @id"),
        Err(CliError::No)
    ));
}

#[test]
fn accepts_valid_query() {
    assert!(check_js("Q = (identifier) @id").is_ok());
}
