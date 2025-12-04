use crate::parser::parse;

#[test]
fn error_json_serialization() {
    let input = "(identifier) @foo.bar";
    let result = parse(input).unwrap();
    let errors = result.diagnostics();

    assert_eq!(errors.len(), 1);
    let json = serde_json::to_string_pretty(errors.iter().next().unwrap()).unwrap();

    insta::assert_snapshot!(json, @r#"
    {
      "severity": "error",
      "stage": "parse",
      "range": {
        "start": 14,
        "end": 21
      },
      "message": "capture names cannot contain dots",
      "fix": {
        "replacement": "foo_bar",
        "description": "captures become struct fields; use @foo_bar instead"
      }
    }
    "#);
}

#[test]
fn error_json_serialization_no_fix() {
    let input = "(identifier) @";
    let result = parse(input).unwrap();
    let errors = result.diagnostics();

    assert_eq!(errors.len(), 1);
    let json = serde_json::to_string_pretty(errors.iter().next().unwrap()).unwrap();

    insta::assert_snapshot!(json, @r#"
    {
      "severity": "error",
      "stage": "parse",
      "range": {
        "start": 14,
        "end": 14
      },
      "message": "expected capture name after '@'"
    }
    "#);
}
