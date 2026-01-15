//! Output value types for materialization.

use arborium_tree_sitter::Node;
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Serialize, Serializer};

use plotnik_core::Colors;

/// Lifetime-free node handle for output.
///
/// Captures enough information to represent a node without holding
/// a reference to the tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeHandle {
    /// Node kind name (e.g., "identifier", "number").
    pub kind: String,
    /// Source text of the node.
    pub text: String,
    /// Byte span [start, end).
    pub span: (u32, u32),
}

impl NodeHandle {
    /// Create from a tree-sitter node and source text.
    pub fn from_node(node: Node<'_>, source: &str) -> Self {
        let text = node
            .utf8_text(source.as_bytes())
            .expect("node text extraction failed")
            .to_owned();
        Self {
            kind: node.kind().to_owned(),
            text,
            span: (node.start_byte() as u32, node.end_byte() as u32),
        }
    }
}

impl Serialize for NodeHandle {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("NodeHandle", 3)?;
        s.serialize_field("kind", &self.kind)?;
        s.serialize_field("text", &self.text)?;
        s.serialize_field("span", &[self.span.0, self.span.1])?;
        s.end()
    }
}

/// Self-contained output value.
///
/// `Object` uses `Vec<(String, Value)>` to preserve field order from type metadata.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    String(String),
    Node(NodeHandle),
    Array(Vec<Value>),
    /// Object with ordered fields.
    Object(Vec<(String, Value)>),
    /// Tagged union. `data` is None for Void payloads.
    Tagged {
        tag: String,
        data: Option<Box<Value>>,
    },
}

impl Serialize for Value {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Value::Null => serializer.serialize_none(),
            Value::String(s) => serializer.serialize_str(s),
            Value::Node(h) => h.serialize(serializer),
            Value::Array(arr) => {
                let mut seq = serializer.serialize_seq(Some(arr.len()))?;
                for item in arr {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            Value::Object(fields) => {
                let mut map = serializer.serialize_map(Some(fields.len()))?;
                for (key, value) in fields {
                    map.serialize_entry(key, value)?;
                }
                map.end()
            }
            Value::Tagged { tag, data } => {
                let len = if data.is_some() { 2 } else { 1 };
                let mut map = serializer.serialize_map(Some(len))?;
                map.serialize_entry("$tag", tag)?;
                if let Some(d) = data {
                    map.serialize_entry("$data", d)?;
                }
                map.end()
            }
        }
    }
}

impl Value {
    /// Format value as colored JSON.
    ///
    /// Color scheme (jq-inspired):
    /// - Keys: Blue
    /// - String values: Green
    /// - Numbers, booleans: Normal
    /// - null: Dim
    /// - Structure `{}[]:,`: Dim
    pub fn format(&self, pretty: bool, colors: Colors) -> String {
        let mut out = String::new();
        format_value(&mut out, self, &colors, pretty, 0);
        out
    }
}

fn format_value(out: &mut String, value: &Value, c: &Colors, pretty: bool, indent: usize) {
    match value {
        Value::Null => {
            out.push_str(c.dim);
            out.push_str("null");
            out.push_str(c.reset);
        }
        Value::String(s) => {
            out.push_str(c.green);
            out.push('"');
            out.push_str(&escape_json_string(s));
            out.push('"');
            out.push_str(c.reset);
        }
        Value::Node(h) => {
            format_node_handle(out, h, c, pretty, indent);
        }
        Value::Array(arr) => {
            format_array(out, arr, c, pretty, indent);
        }
        Value::Object(fields) => {
            format_object(out, fields, c, pretty, indent);
        }
        Value::Tagged { tag, data } => {
            format_tagged(out, tag, data, c, pretty, indent);
        }
    }
}

fn format_node_handle(out: &mut String, h: &NodeHandle, c: &Colors, pretty: bool, indent: usize) {
    out.push_str(c.dim);
    out.push('{');
    out.push_str(c.reset);

    let field_indent = if pretty { indent + 2 } else { 0 };

    // Field 1: "kind"
    if pretty {
        out.push('\n');
        out.push_str(&" ".repeat(field_indent));
    }
    out.push_str(c.blue);
    out.push_str("\"kind\"");
    out.push_str(c.reset);
    out.push_str(c.dim);
    out.push(':');
    out.push_str(c.reset);
    if pretty {
        out.push(' ');
    }
    out.push_str(c.green);
    out.push('"');
    out.push_str(&escape_json_string(&h.kind));
    out.push('"');
    out.push_str(c.reset);

    // Field 2: "text"
    out.push_str(c.dim);
    out.push(',');
    out.push_str(c.reset);
    if pretty {
        out.push('\n');
        out.push_str(&" ".repeat(field_indent));
    }
    out.push_str(c.blue);
    out.push_str("\"text\"");
    out.push_str(c.reset);
    out.push_str(c.dim);
    out.push(':');
    out.push_str(c.reset);
    if pretty {
        out.push(' ');
    }
    out.push_str(c.green);
    out.push('"');
    out.push_str(&escape_json_string(&h.text));
    out.push('"');
    out.push_str(c.reset);

    // Field 3: "span"
    out.push_str(c.dim);
    out.push(',');
    out.push_str(c.reset);
    if pretty {
        out.push('\n');
        out.push_str(&" ".repeat(field_indent));
    }
    out.push_str(c.blue);
    out.push_str("\"span\"");
    out.push_str(c.reset);
    out.push_str(c.dim);
    out.push(':');
    out.push_str(c.reset);
    if pretty {
        out.push(' ');
    }
    out.push_str(c.dim);
    out.push('[');
    out.push_str(c.reset);
    out.push_str(&h.span.0.to_string());
    out.push_str(c.dim);
    out.push_str(", ");
    out.push_str(c.reset);
    out.push_str(&h.span.1.to_string());
    out.push_str(c.dim);
    out.push(']');
    out.push_str(c.reset);

    if pretty {
        out.push('\n');
        out.push_str(&" ".repeat(indent));
    }

    out.push_str(c.dim);
    out.push('}');
    out.push_str(c.reset);
}

fn format_array(out: &mut String, arr: &[Value], c: &Colors, pretty: bool, indent: usize) {
    out.push_str(c.dim);
    out.push('[');
    out.push_str(c.reset);

    if arr.is_empty() {
        out.push_str(c.dim);
        out.push(']');
        out.push_str(c.reset);
        return;
    }

    let elem_indent = if pretty { indent + 2 } else { 0 };

    for (i, item) in arr.iter().enumerate() {
        if i > 0 {
            out.push_str(c.dim);
            out.push(',');
            out.push_str(c.reset);
        }

        if pretty {
            out.push('\n');
            out.push_str(&" ".repeat(elem_indent));
        }

        format_value(out, item, c, pretty, elem_indent);
    }

    if pretty {
        out.push('\n');
        out.push_str(&" ".repeat(indent));
    }

    out.push_str(c.dim);
    out.push(']');
    out.push_str(c.reset);
}

fn format_object(
    out: &mut String,
    fields: &[(String, Value)],
    c: &Colors,
    pretty: bool,
    indent: usize,
) {
    out.push_str(c.dim);
    out.push('{');
    out.push_str(c.reset);

    if fields.is_empty() {
        out.push_str(c.dim);
        out.push('}');
        out.push_str(c.reset);
        return;
    }

    let field_indent = if pretty { indent + 2 } else { 0 };

    for (i, (key, value)) in fields.iter().enumerate() {
        if i > 0 {
            out.push_str(c.dim);
            out.push(',');
            out.push_str(c.reset);
        }

        if pretty {
            out.push('\n');
            out.push_str(&" ".repeat(field_indent));
        }

        // Key in blue
        out.push_str(c.blue);
        out.push('"');
        out.push_str(&escape_json_string(key));
        out.push('"');
        out.push_str(c.reset);

        out.push_str(c.dim);
        out.push(':');
        out.push_str(c.reset);

        if pretty {
            out.push(' ');
        }

        format_value(out, value, c, pretty, field_indent);
    }

    if pretty {
        out.push('\n');
        out.push_str(&" ".repeat(indent));
    }

    out.push_str(c.dim);
    out.push('}');
    out.push_str(c.reset);
}

fn format_tagged(
    out: &mut String,
    tag: &str,
    data: &Option<Box<Value>>,
    c: &Colors,
    pretty: bool,
    indent: usize,
) {
    out.push_str(c.dim);
    out.push('{');
    out.push_str(c.reset);

    let field_indent = if pretty { indent + 2 } else { 0 };

    if pretty {
        out.push('\n');
        out.push_str(&" ".repeat(field_indent));
    }

    // $tag key in blue
    out.push_str(c.blue);
    out.push_str("\"$tag\"");
    out.push_str(c.reset);

    out.push_str(c.dim);
    out.push(':');
    out.push_str(c.reset);

    if pretty {
        out.push(' ');
    }

    // Tag value is green (string)
    out.push_str(c.green);
    out.push('"');
    out.push_str(&escape_json_string(tag));
    out.push('"');
    out.push_str(c.reset);

    // Only emit $data if present (Void payloads omit it)
    if let Some(d) = data {
        out.push_str(c.dim);
        out.push(',');
        out.push_str(c.reset);

        if pretty {
            out.push('\n');
            out.push_str(&" ".repeat(field_indent));
        }

        // $data key in blue
        out.push_str(c.blue);
        out.push_str("\"$data\"");
        out.push_str(c.reset);

        out.push_str(c.dim);
        out.push(':');
        out.push_str(c.reset);

        if pretty {
            out.push(' ');
        }

        format_value(out, d, c, pretty, field_indent);
    }

    if pretty {
        out.push('\n');
        out.push_str(&" ".repeat(indent));
    }

    out.push_str(c.dim);
    out.push('}');
    out.push_str(c.reset);
}

fn escape_json_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            c if c.is_control() => {
                result.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => result.push(c),
        }
    }
    result
}
