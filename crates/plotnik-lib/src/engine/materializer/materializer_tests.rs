use plotnik_langs::javascript;

use crate::engine::effect_stream::{CapturedNode, EffectStream};
use crate::engine::materializer::Materializer;
use crate::engine::value::Value;
use crate::ir::EffectOp;

fn capture_node<'tree>(
    tree: &'tree tree_sitter::Tree,
    source: &'tree str,
    index: usize,
) -> CapturedNode<'tree> {
    let mut cursor = tree.walk();
    cursor.goto_first_child();
    for _ in 0..index {
        cursor.goto_next_sibling();
    }
    CapturedNode::new(cursor.node(), source)
}

#[test]
fn materialize_simple_object() {
    let lang = javascript();
    let source = "a; b;";
    let tree = lang.parse(source);

    let node0 = capture_node(&tree, source, 0);
    let node1 = capture_node(&tree, source, 1);

    let mut stream = EffectStream::new();
    stream.push_op(EffectOp::StartObject);
    stream.push_op(EffectOp::CaptureNode);
    stream.push_captured_node(node0);
    stream.push_op(EffectOp::Field(10));
    stream.push_op(EffectOp::CaptureNode);
    stream.push_captured_node(node1);
    stream.push_op(EffectOp::Field(20));
    stream.push_op(EffectOp::EndObject);

    let value = Materializer::materialize(&stream);

    match value {
        Value::Object(map) => {
            assert_eq!(map.len(), 2);
            assert!(map.contains_key(&10));
            assert!(map.contains_key(&20));
        }
        _ => panic!("expected Object"),
    }
}

#[test]
fn materialize_simple_array() {
    let lang = javascript();
    let source = "a; b;";
    let tree = lang.parse(source);

    let node0 = capture_node(&tree, source, 0);
    let node1 = capture_node(&tree, source, 1);

    let mut stream = EffectStream::new();
    stream.push_op(EffectOp::StartArray);
    stream.push_op(EffectOp::CaptureNode);
    stream.push_captured_node(node0);
    stream.push_op(EffectOp::PushElement);
    stream.push_op(EffectOp::CaptureNode);
    stream.push_captured_node(node1);
    stream.push_op(EffectOp::PushElement);
    stream.push_op(EffectOp::EndArray);

    let value = Materializer::materialize(&stream);

    match value {
        Value::Array(arr) => {
            assert_eq!(arr.len(), 2);
            assert!(matches!(arr[0], Value::Node(_)));
            assert!(matches!(arr[1], Value::Node(_)));
        }
        _ => panic!("expected Array"),
    }
}

#[test]
fn materialize_object_with_optional_field() {
    let lang = javascript();
    let source = "a;";
    let tree = lang.parse(source);

    let node0 = capture_node(&tree, source, 0);

    let mut stream = EffectStream::new();
    stream.push_op(EffectOp::StartObject);
    stream.push_op(EffectOp::CaptureNode);
    stream.push_captured_node(node0);
    stream.push_op(EffectOp::Field(10));
    stream.push_op(EffectOp::ClearCurrent);
    stream.push_op(EffectOp::Field(30));
    stream.push_op(EffectOp::EndObject);

    let value = Materializer::materialize(&stream);

    match value {
        Value::Object(map) => {
            assert_eq!(map.len(), 2);
            assert!(matches!(map.get(&10), Some(Value::Node(_))));
            assert!(matches!(map.get(&30), Some(Value::Null)));
        }
        _ => panic!("expected Object"),
    }
}

#[test]
fn materialize_variant() {
    let lang = javascript();
    let source = "a;";
    let tree = lang.parse(source);

    let node0 = capture_node(&tree, source, 0);

    let mut stream = EffectStream::new();
    stream.push_op(EffectOp::StartVariant(100));
    stream.push_op(EffectOp::CaptureNode);
    stream.push_captured_node(node0);
    stream.push_op(EffectOp::EndVariant);

    let value = Materializer::materialize(&stream);

    match value {
        Value::Variant { tag, value } => {
            assert_eq!(tag, 100);
            assert!(matches!(*value, Value::Node(_)));
        }
        _ => panic!("expected Variant"),
    }
}

#[test]
fn materialize_to_string() {
    let lang = javascript();
    let source = "hello";
    let tree = lang.parse(source);

    // Get the identifier node (program -> expression_statement -> identifier)
    let root = tree.root_node();
    let expr_stmt = root.child(0).unwrap();
    let ident = expr_stmt.child(0).unwrap();
    let node = CapturedNode::new(ident, source);

    let mut stream = EffectStream::new();
    stream.push_op(EffectOp::CaptureNode);
    stream.push_captured_node(node);
    stream.push_op(EffectOp::ToString);

    let value = Materializer::materialize(&stream);

    assert_eq!(value, Value::String("hello".to_string()));
}
