//! Output value types for materialization.

use std::fmt::Write as _;

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
    /// Node kind name (e.g., "identifier"). Tree-sitter kind names live in the
    /// grammar's static symbol table, hence `&'static`.
    pub kind: &'static str,
    /// Source text of the node.
    pub text: String,
    /// Byte span [start, end).
    pub span: (u32, u32),
}

impl NodeHandle {
    pub fn from_node(node: Node<'_>, source: &str) -> Self {
        let text = node
            .utf8_text(source.as_bytes())
            .expect("node source text must be valid UTF-8")
            .to_owned();
        Self {
            kind: node.kind(),
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
/// `Struct` uses `Vec<(String, Value)>` to preserve field order from type metadata.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Node(NodeHandle),
    Array(Vec<Value>),
    /// Struct with ordered fields.
    Struct(Vec<(String, Value)>),
    /// Enum variant. `data` is None for Void payloads.
    Enum {
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
            Value::Node(h) => h.serialize(serializer),
            Value::Array(arr) => {
                let mut seq = serializer.serialize_seq(Some(arr.len()))?;
                for item in arr {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            Value::Struct(fields) => {
                let mut map = serializer.serialize_map(Some(fields.len()))?;
                for (key, value) in fields {
                    map.serialize_entry(key, value)?;
                }
                map.end()
            }
            Value::Enum { tag, data } => {
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
        let mut ctx = FormatCtx {
            out: &mut out,
            colors: &colors,
            pretty,
        };
        format_value(&mut ctx, self, 0);
        out
    }
}

/// `indent` varies per recursion step; the rest is shared state threaded through every call.
struct FormatCtx<'a> {
    out: &'a mut String,
    colors: &'a Colors,
    pretty: bool,
}

fn format_value(ctx: &mut FormatCtx<'_>, value: &Value, indent: usize) {
    match value {
        Value::Null => {
            let c = ctx.colors;
            ctx.out.push_str(c.dim);
            ctx.out.push_str("null");
            ctx.out.push_str(c.reset);
        }
        Value::Node(h) => {
            format_node_handle(ctx, h, indent);
        }
        Value::Array(arr) => {
            format_array(ctx, arr, indent);
        }
        Value::Struct(fields) => {
            format_struct(ctx, fields, indent);
        }
        Value::Enum { tag, data } => {
            format_enum(ctx, tag, data, indent);
        }
    }
}

fn format_node_handle(ctx: &mut FormatCtx<'_>, h: &NodeHandle, indent: usize) {
    let c = ctx.colors;
    let pretty = ctx.pretty;
    let out = &mut *ctx.out;
    out.push_str(c.dim);
    out.push('{');
    out.push_str(c.reset);

    let field_indent = if pretty { indent + 2 } else { 0 };

    if pretty {
        out.push('\n');
        push_indent(out, field_indent);
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
    escape_json_into(out, h.kind);
    out.push('"');
    out.push_str(c.reset);

    out.push_str(c.dim);
    out.push(',');
    out.push_str(c.reset);
    if pretty {
        out.push('\n');
        push_indent(out, field_indent);
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
    escape_json_into(out, &h.text);
    out.push('"');
    out.push_str(c.reset);

    out.push_str(c.dim);
    out.push(',');
    out.push_str(c.reset);
    if pretty {
        out.push('\n');
        push_indent(out, field_indent);
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
    let _ = write!(out, "{}", h.span.0);
    out.push_str(c.dim);
    out.push_str(", ");
    out.push_str(c.reset);
    let _ = write!(out, "{}", h.span.1);
    out.push_str(c.dim);
    out.push(']');
    out.push_str(c.reset);

    if pretty {
        out.push('\n');
        push_indent(out, indent);
    }

    out.push_str(c.dim);
    out.push('}');
    out.push_str(c.reset);
}

fn format_array(ctx: &mut FormatCtx<'_>, arr: &[Value], indent: usize) {
    let c = ctx.colors;
    let pretty = ctx.pretty;
    ctx.out.push_str(c.dim);
    ctx.out.push('[');
    ctx.out.push_str(c.reset);

    if arr.is_empty() {
        ctx.out.push_str(c.dim);
        ctx.out.push(']');
        ctx.out.push_str(c.reset);
        return;
    }

    let elem_indent = if pretty { indent + 2 } else { 0 };

    for (i, item) in arr.iter().enumerate() {
        if i > 0 {
            ctx.out.push_str(c.dim);
            ctx.out.push(',');
            ctx.out.push_str(c.reset);
        }

        if pretty {
            ctx.out.push('\n');
            push_indent(ctx.out, elem_indent);
        }

        format_value(ctx, item, elem_indent);
    }

    if pretty {
        ctx.out.push('\n');
        push_indent(ctx.out, indent);
    }

    ctx.out.push_str(c.dim);
    ctx.out.push(']');
    ctx.out.push_str(c.reset);
}

fn format_struct(ctx: &mut FormatCtx<'_>, fields: &[(String, Value)], indent: usize) {
    let c = ctx.colors;
    let pretty = ctx.pretty;
    ctx.out.push_str(c.dim);
    ctx.out.push('{');
    ctx.out.push_str(c.reset);

    if fields.is_empty() {
        ctx.out.push_str(c.dim);
        ctx.out.push('}');
        ctx.out.push_str(c.reset);
        return;
    }

    let field_indent = if pretty { indent + 2 } else { 0 };

    for (i, (key, value)) in fields.iter().enumerate() {
        if i > 0 {
            ctx.out.push_str(c.dim);
            ctx.out.push(',');
            ctx.out.push_str(c.reset);
        }

        if pretty {
            ctx.out.push('\n');
            push_indent(ctx.out, field_indent);
        }

        ctx.out.push_str(c.blue);
        ctx.out.push('"');
        escape_json_into(ctx.out, key);
        ctx.out.push('"');
        ctx.out.push_str(c.reset);

        ctx.out.push_str(c.dim);
        ctx.out.push(':');
        ctx.out.push_str(c.reset);

        if pretty {
            ctx.out.push(' ');
        }

        format_value(ctx, value, field_indent);
    }

    if pretty {
        ctx.out.push('\n');
        push_indent(ctx.out, indent);
    }

    ctx.out.push_str(c.dim);
    ctx.out.push('}');
    ctx.out.push_str(c.reset);
}

fn format_enum(ctx: &mut FormatCtx<'_>, tag: &str, data: &Option<Box<Value>>, indent: usize) {
    let c = ctx.colors;
    let pretty = ctx.pretty;
    ctx.out.push_str(c.dim);
    ctx.out.push('{');
    ctx.out.push_str(c.reset);

    let field_indent = if pretty { indent + 2 } else { 0 };

    if pretty {
        ctx.out.push('\n');
        push_indent(ctx.out, field_indent);
    }

    ctx.out.push_str(c.blue);
    ctx.out.push_str("\"$tag\"");
    ctx.out.push_str(c.reset);

    ctx.out.push_str(c.dim);
    ctx.out.push(':');
    ctx.out.push_str(c.reset);

    if pretty {
        ctx.out.push(' ');
    }

    ctx.out.push_str(c.green);
    ctx.out.push('"');
    escape_json_into(ctx.out, tag);
    ctx.out.push('"');
    ctx.out.push_str(c.reset);

    // Void payloads have no $data field.
    if let Some(d) = data {
        ctx.out.push_str(c.dim);
        ctx.out.push(',');
        ctx.out.push_str(c.reset);

        if pretty {
            ctx.out.push('\n');
            push_indent(ctx.out, field_indent);
        }

        ctx.out.push_str(c.blue);
        ctx.out.push_str("\"$data\"");
        ctx.out.push_str(c.reset);

        ctx.out.push_str(c.dim);
        ctx.out.push(':');
        ctx.out.push_str(c.reset);

        if pretty {
            ctx.out.push(' ');
        }

        format_value(ctx, d, field_indent);
    }

    if pretty {
        ctx.out.push('\n');
        push_indent(ctx.out, indent);
    }

    ctx.out.push_str(c.dim);
    ctx.out.push('}');
    ctx.out.push_str(c.reset);
}

/// Escape `s` as a JSON string body, appending to `out`.
fn escape_json_into(out: &mut String, s: &str) {
    let needs_escape = |c: char| matches!(c, '"' | '\\' | '\n' | '\r' | '\t') || c.is_control();

    // Copy the unescaped prefix in one shot, then escape from the first
    // offending char onward.
    let Some((split, _)) = s.char_indices().find(|&(_, c)| needs_escape(c)) else {
        out.push_str(s);
        return;
    };

    out.push_str(&s[..split]);
    for ch in s[split..].chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
}

fn push_indent(out: &mut String, n: usize) {
    for _ in 0..n {
        out.push(' ');
    }
}
