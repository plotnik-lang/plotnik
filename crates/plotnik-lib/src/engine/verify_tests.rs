//! Tests for debug type verification.

use crate::Colors;
use crate::QueryBuilder;
use crate::bytecode::{Module, TypeId};
use crate::emit::emit_linked;
use crate::engine::value::{NodeHandle, Value};

use super::debug_verify_type;

/// Build a module from a query string and return with its first entrypoint's result type.
fn build_module(query: &str) -> (Module, TypeId) {
    let lang = plotnik_langs::javascript();
    let query_obj = QueryBuilder::one_liner(query)
        .parse()
        .expect("parse failed")
        .analyze()
        .link(&lang);
    assert!(query_obj.is_valid(), "query should be valid");
    let bytecode = emit_linked(&query_obj).expect("emit failed");
    let module = Module::load(&bytecode).expect("decode failed");
    let declared_type = module.entrypoints().get(0).result_type;
    (module, declared_type)
}

fn make_node() -> Value {
    Value::Node(NodeHandle {
        kind: "identifier".to_string(),
        text: "x".to_string(),
        span: (0, 1),
    })
}

#[test]
fn verify_valid_node() {
    let (module, declared_type) = build_module("Q = (identifier) @id");
    let value = Value::Object(vec![("id".to_string(), make_node())]);

    // Should not panic
    debug_verify_type(&value, declared_type, &module, Colors::OFF);
}

#[test]
fn verify_valid_optional_present() {
    let (module, declared_type) = build_module("Q = (identifier)? @id");
    let value = Value::Object(vec![("id".to_string(), make_node())]);

    debug_verify_type(&value, declared_type, &module, Colors::OFF);
}

#[test]
fn verify_valid_optional_null() {
    let (module, declared_type) = build_module("Q = (identifier)? @id");
    let value = Value::Object(vec![("id".to_string(), Value::Null)]);

    debug_verify_type(&value, declared_type, &module, Colors::OFF);
}

#[test]
fn verify_valid_array() {
    let (module, declared_type) = build_module("Q = (identifier)* @ids");
    let value = Value::Object(vec![(
        "ids".to_string(),
        Value::Array(vec![make_node(), make_node()]),
    )]);

    debug_verify_type(&value, declared_type, &module, Colors::OFF);
}

#[test]
fn verify_valid_empty_array() {
    let (module, declared_type) = build_module("Q = (identifier)* @ids");
    let value = Value::Object(vec![("ids".to_string(), Value::Array(vec![]))]);

    debug_verify_type(&value, declared_type, &module, Colors::OFF);
}

#[test]
fn verify_valid_enum() {
    let (module, declared_type) = build_module("Q = [A: (identifier) @x  B: (number) @y]");
    let value = Value::Tagged {
        tag: "A".to_string(),
        data: Some(Box::new(Value::Object(vec![(
            "x".to_string(),
            make_node(),
        )]))),
    };

    debug_verify_type(&value, declared_type, &module, Colors::OFF);
}

#[test]
fn verify_valid_enum_void_variant() {
    let (module, declared_type) = build_module("Q = [A: (identifier) @x  B: (number)]");
    let value = Value::Tagged {
        tag: "B".to_string(),
        data: None,
    };

    debug_verify_type(&value, declared_type, &module, Colors::OFF);
}

#[test]
fn verify_valid_string() {
    let (module, declared_type) = build_module("Q = (identifier) @id :: string");
    let value = Value::Object(vec![("id".to_string(), Value::String("foo".to_string()))]);

    debug_verify_type(&value, declared_type, &module, Colors::OFF);
}

#[test]
#[should_panic(expected = "BUG:")]
fn verify_invalid_node_is_string() {
    let (module, declared_type) = build_module("Q = (identifier) @id");

    // id should be Node, but we provide string
    let value = Value::Object(vec![("id".to_string(), Value::String("wrong".to_string()))]);

    debug_verify_type(&value, declared_type, &module, Colors::OFF);
}

#[test]
#[should_panic(expected = "BUG:")]
fn verify_invalid_missing_required_field() {
    let (module, declared_type) = build_module("Q = {(identifier) @a (number) @b}");

    // Missing field 'b'
    let value = Value::Object(vec![("a".to_string(), make_node())]);

    debug_verify_type(&value, declared_type, &module, Colors::OFF);
}

#[test]
#[should_panic(expected = "BUG:")]
fn verify_invalid_array_element_wrong_type() {
    let (module, declared_type) = build_module("Q = (identifier)* @ids");

    // Array element is string instead of Node
    let value = Value::Object(vec![(
        "ids".to_string(),
        Value::Array(vec![make_node(), Value::String("oops".to_string())]),
    )]);

    debug_verify_type(&value, declared_type, &module, Colors::OFF);
}

#[test]
#[should_panic(expected = "BUG:")]
fn verify_invalid_non_empty_array_is_empty() {
    let (module, declared_type) = build_module("Q = (identifier)+ @ids");

    // Non-empty array but we provide empty
    let value = Value::Object(vec![("ids".to_string(), Value::Array(vec![]))]);

    debug_verify_type(&value, declared_type, &module, Colors::OFF);
}

#[test]
#[should_panic(expected = "BUG:")]
fn verify_invalid_enum_unknown_variant() {
    let (module, declared_type) = build_module("Q = [A: (identifier) @x  B: (number) @y]");

    let value = Value::Tagged {
        tag: "C".to_string(), // Unknown variant
        data: None,
    };

    debug_verify_type(&value, declared_type, &module, Colors::OFF);
}

#[test]
#[should_panic(expected = "BUG:")]
fn verify_invalid_enum_void_with_data() {
    let (module, declared_type) = build_module("Q = [A: (identifier) @x  B: (number)]");

    // Void variant B has data when it shouldn't
    let value = Value::Tagged {
        tag: "B".to_string(),
        data: Some(Box::new(Value::Object(vec![]))),
    };

    debug_verify_type(&value, declared_type, &module, Colors::OFF);
}

#[test]
#[should_panic(expected = "BUG:")]
fn verify_invalid_enum_non_void_missing_data() {
    let (module, declared_type) = build_module("Q = [A: (identifier) @x  B: (number) @y]");

    // Non-void variant A missing data
    let value = Value::Tagged {
        tag: "A".to_string(),
        data: None,
    };

    debug_verify_type(&value, declared_type, &module, Colors::OFF);
}

#[test]
#[should_panic(expected = "BUG:")]
fn verify_invalid_object_vs_array() {
    let (module, declared_type) = build_module("Q = (identifier) @id");

    // Type says object, value is array
    let value = Value::Array(vec![make_node()]);

    debug_verify_type(&value, declared_type, &module, Colors::OFF);
}
