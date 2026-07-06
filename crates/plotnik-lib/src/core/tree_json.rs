//! JSON rendering for parsed source trees.

use serde_json::{Map, Value};

/// Serialize a tree-sitter tree as editor-friendly JSON.
///
/// Anonymous nodes are omitted unless `raw` is set, matching the CLI's text AST
/// view. The walk is iterative so deeply nested source files cannot overflow the
/// native stack while building diagnostics or playground payloads.
pub fn tree_to_json(tree: &tree_sitter::Tree, _source: &str, raw: bool) -> Value {
    let mut stack = vec![Frame::new(tree.root_node(), None, raw)];

    loop {
        let next_child = {
            let frame = stack.last_mut().expect("tree frame stack cannot be empty");
            if frame.next_child >= frame.child_edges.len() {
                None
            } else {
                let child = frame.child_edges[frame.next_child];
                frame.next_child += 1;
                Some(child)
            }
        };

        if let Some((node, field)) = next_child {
            stack.push(Frame::new(node, field, raw));
            continue;
        }

        let frame = stack.pop().expect("tree frame stack cannot be empty");
        let value = frame.into_value();
        let Some(parent) = stack.last_mut() else {
            return value;
        };
        parent.child_values.push(value);
    }
}

struct Frame<'t> {
    node: tree_sitter::Node<'t>,
    field: Option<&'t str>,
    child_edges: Vec<(tree_sitter::Node<'t>, Option<&'t str>)>,
    next_child: usize,
    child_values: Vec<Value>,
}

impl<'t> Frame<'t> {
    fn new(node: tree_sitter::Node<'t>, field: Option<&'t str>, raw: bool) -> Self {
        let child_edges = collect_children(node, raw);
        let child_values = Vec::with_capacity(child_edges.len());
        Self {
            node,
            field,
            child_edges,
            next_child: 0,
            child_values,
        }
    }

    fn into_value(self) -> Value {
        let range = self.node.byte_range();
        let mut object = Map::new();
        object.insert(
            "kind".to_string(),
            Value::String(self.node.kind().to_string()),
        );
        object.insert("named".to_string(), Value::Bool(self.node.is_named()));
        object.insert(
            "range".to_string(),
            Value::Array(vec![offset_value(range.start), offset_value(range.end)]),
        );
        if let Some(field) = self.field {
            object.insert("field".to_string(), Value::String(field.to_string()));
        }
        object.insert("children".to_string(), Value::Array(self.child_values));
        Value::Object(object)
    }
}

fn collect_children<'t>(
    node: tree_sitter::Node<'t>,
    raw: bool,
) -> Vec<(tree_sitter::Node<'t>, Option<&'t str>)> {
    let mut cursor = node.walk();
    let mut result = Vec::new();
    if !cursor.goto_first_child() {
        return result;
    }

    loop {
        let child = cursor.node();
        if raw || child.is_named() {
            result.push((child, cursor.field_name()));
        }
        if !cursor.goto_next_sibling() {
            break;
        }
    }
    result
}

fn offset_value(offset: usize) -> Value {
    Value::from(u64::try_from(offset).expect("tree byte offset fits in u64"))
}
