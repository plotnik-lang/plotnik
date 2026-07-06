#![cfg(feature = "lang-javascript")]

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
        .entrypoint("Second")
        .expect("last definition is an entrypoint candidate");
    assert_eq!(plan.entrypoint, expected);
}

#[test]
fn explicit_entry_overrides_the_default() {
    let plan = exec_plan(TWO_DEFS, Some("First"));
    let expected = plan
        .module
        .entrypoint("First")
        .expect("named definition is an entrypoint candidate");
    assert_eq!(plan.entrypoint, expected);
}
