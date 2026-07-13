#![cfg(feature = "lang-javascript")]

use std::path::Path;

use super::run_common::{self, ExecPlan, ExecRequest};

const TWO_DEFS: &str = "First = (identifier) @a\nSecond = (number) @b";

fn exec_plan(query: &str, entry: Option<&str>) -> ExecPlan {
    run_common::plan_exec(ExecRequest {
        query_path: None,
        query_text: Some(query),
        source_path: None,
        source_text: Some("let x = 1"),
        lang: Some("javascript"),
        entry,
        color: false,
        inspection: false,
    })
    .expect("query compiles and an entrypoint resolves")
}

#[test]
fn defaults_to_last_definition_when_entry_omitted() {
    let plan = exec_plan(TWO_DEFS, None);
    let expected = plan
        .module
        .entry_point("Second")
        .expect("last selectable definition can be an entry point");
    assert_eq!(plan.entrypoint, expected);
}

#[test]
fn explicit_entry_overrides_the_default() {
    let plan = exec_plan(TWO_DEFS, Some("First"));
    let expected = plan
        .module
        .entry_point("First")
        .expect("named selectable definition can be an entry point");
    assert_eq!(plan.entrypoint, expected);
}

#[test]
fn query_supplied_as_text_and_path_is_rejected() {
    let err = run_common::reject_ambiguous_inputs(
        Some("Q = (id)"),
        Some(Path::new("q.ptk")),
        None,
        Some(Path::new("app.js")),
    )
    .expect_err("a query given both inline and positionally is ambiguous");

    assert!(format!("{err:?}").contains("query supplied twice"));
}

#[test]
fn source_supplied_as_text_and_path_is_rejected() {
    let err = run_common::reject_ambiguous_inputs(
        Some("Q = (id)"),
        None,
        Some("let x = 1"),
        Some(Path::new("app.js")),
    )
    .expect_err("a source given both inline and positionally is ambiguous");

    assert!(format!("{err:?}").contains("source supplied twice"));
}

#[test]
fn inline_query_with_positional_source_is_allowed() {
    // The `-q TEXT source.js` shape: query inline, source positional.
    run_common::reject_ambiguous_inputs(Some("Q = (id)"), None, None, Some(Path::new("app.js")))
        .expect("query inline plus a source path is unambiguous");
}

#[test]
fn both_inputs_positional_is_allowed() {
    run_common::reject_ambiguous_inputs(
        None,
        Some(Path::new("q.ptk")),
        None,
        Some(Path::new("app.js")),
    )
    .expect("query path plus source path is unambiguous");
}
