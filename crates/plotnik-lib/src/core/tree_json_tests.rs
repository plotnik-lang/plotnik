use arborium_tree_sitter::{Language as TsLanguage, Parser as TsParser, Tree};
use serde_json::{Map, Value};

use super::tree_to_json;

#[test]
fn tree_to_json_named_mode_omits_anonymous_nodes_and_keeps_fields() {
    let source = "let x = 1;";
    let tree = parse_js(source);

    let json = tree_to_json(&tree, source, false);

    assert_eq!(field_str(&json, "kind"), Some("program"));
    assert_eq!(field_bool(&json, "named"), Some(true));
    assert!(has_node(&json, &|object| {
        field_str_object(object, "kind") == Some("identifier")
            && field_str_object(object, "field") == Some("name")
            && range_is(object, 4, 5)
    }));
    assert!(has_node(&json, &|object| {
        field_str_object(object, "kind") == Some("number")
            && field_str_object(object, "field") == Some("value")
            && range_is(object, 8, 9)
    }));
    assert!(!has_node(&json, &|object| {
        field_bool_object(object, "named") == Some(false)
    }));
}

#[test]
fn tree_to_json_raw_mode_includes_anonymous_nodes() {
    let source = "let x = 1;";
    let tree = parse_js(source);

    let json = tree_to_json(&tree, source, true);

    assert!(has_node(&json, &|object| {
        field_str_object(object, "kind") == Some("=")
            && field_bool_object(object, "named") == Some(false)
            && range_is(object, 6, 7)
    }));
    assert!(has_node(&json, &|object| {
        field_str_object(object, "kind") == Some(";")
            && field_bool_object(object, "named") == Some(false)
            && range_is(object, 9, 10)
    }));
}

fn parse_js(source: &str) -> Tree {
    let mut parser = TsParser::new();
    let lang: TsLanguage = arborium_javascript::language().into();
    parser.set_language(&lang).expect("set javascript language");
    parser.parse(source, None).expect("parse javascript source")
}

fn has_node(value: &Value, matches: &dyn Fn(&Map<String, Value>) -> bool) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    if matches(object) {
        return true;
    }
    let Some(children) = object.get("children").and_then(Value::as_array) else {
        return false;
    };
    children.iter().any(|child| has_node(child, matches))
}

fn range_is(object: &Map<String, Value>, start: u64, end: u64) -> bool {
    let Some(range) = object.get("range").and_then(Value::as_array) else {
        return false;
    };
    range.len() == 2 && range[0].as_u64() == Some(start) && range[1].as_u64() == Some(end)
}

fn field_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.as_object()?.get(key)?.as_str()
}

fn field_bool(value: &Value, key: &str) -> Option<bool> {
    value.as_object()?.get(key)?.as_bool()
}

fn field_str_object<'a>(object: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    object.get(key)?.as_str()
}

fn field_bool_object(object: &Map<String, Value>, key: &str) -> Option<bool> {
    object.get(key)?.as_bool()
}
