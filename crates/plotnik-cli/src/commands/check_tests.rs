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
fn accepts_enum_zero_width_branch_in_quantifier() {
    // A skippable enum arm in a quantifier (`[A: (comment)? @c]*`) used to emit
    // an unbracketed skip path rejected by bytecode validation;
    // enum bracket dominance makes both arm paths close the enum, so it compiles.
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
fn rejects_value_less_definition() {
    // `Q = .` compiles to a module with no entrypoints.
    assert!(matches!(check_js("Q = ."), Err(CliError::No)));
}

#[test]
fn rejects_dropped_value_less_def_among_valid() {
    // A value-less def is silently dropped from the module even when another def
    // compiles fine; `check` must still flag it instead of exiting 0.
    assert!(matches!(
        check_js("Bad = .\nGood = (identifier) @id"),
        Err(CliError::No)
    ));
}

#[test]
fn accepts_valid_query() {
    assert!(check_js("Q = (identifier) @id").is_ok());
}
