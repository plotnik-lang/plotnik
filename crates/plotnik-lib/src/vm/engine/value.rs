//! Output value types for materialization.

use std::fmt::Write as _;

use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Serialize, Serializer};
use tree_sitter::Node;

use crate::core::Colors;
use crate::core::utils::escape_json_into;

/// Node handle for output, borrowing the query source.
///
/// `text` is a span slice of the source — no copy, no per-node UTF-8
/// re-validation (the source is already `&str`). `kind` points into the
/// grammar's static symbol table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeHandle<'s> {
    /// Node kind name (e.g., "identifier"). Tree-sitter kind names live in the
    /// grammar's static symbol table, hence `&'static`.
    pub kind: &'static str,
    /// Source text of the node.
    pub text: &'s str,
    /// Byte span [start, end).
    pub span: (u32, u32),
}

impl<'s> NodeHandle<'s> {
    pub fn from_node(node: Node<'_>, source: &'s str) -> Self {
        let span = (node.start_byte() as u32, node.end_byte() as u32);
        Self {
            kind: node.kind(),
            text: node_text(source, &node),
            span,
        }
    }
}

/// Slice `node`'s span out of the source. Lives in `plotnik_rt` so generated
/// matchers slice predicate text identically to the VM.
pub(crate) use plotnik_rt::node_text;

impl Serialize for NodeHandle<'_> {
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

/// Self-contained output value, borrowing node text from the query source and
/// member/tag names from the bytecode string table (`'s` must outlive both).
///
/// `Record` uses `Vec<(&str, Value)>` to preserve field order from type metadata.
#[derive(Clone, Debug, PartialEq)]
pub enum Value<'s> {
    Null,
    Node(NodeHandle<'s>),
    Text(&'s str),
    Bool(bool),
    List(Vec<Value<'s>>),
    /// Record with ordered fields.
    Record(Vec<(&'s str, Value<'s>)>),
    /// Variant case. `data` is `None` when the case has no payload.
    Variant {
        tag: &'s str,
        data: Option<Box<Value<'s>>>,
    },
}

impl Drop for Value<'_> {
    fn drop(&mut self) {
        // A captured-recursive query nests values as deep as the match, which can
        // exceed any native-stack budget — so the derived recursive drop could
        // overflow. Dismantle level by level instead: move each node's children
        // onto a heap worklist, so every node drops only after it is childless and
        // its own drop is a leaf.
        let mut worklist: Vec<Value<'_>> = Vec::new();
        take_children(self, &mut worklist);
        while let Some(mut value) = worklist.pop() {
            take_children(&mut value, &mut worklist);
        }
    }
}

/// Move `value`'s direct child values onto `worklist`, leaving `value` childless.
fn take_children<'s>(value: &mut Value<'s>, worklist: &mut Vec<Value<'s>>) {
    match value {
        Value::List(items) => worklist.append(items),
        Value::Record(fields) => worklist.extend(fields.drain(..).map(|(_, v)| v)),
        Value::Variant { data, .. } => {
            if let Some(boxed) = data.take() {
                worklist.push(*boxed);
            }
        }
        Value::Null | Value::Node(_) | Value::Text(_) | Value::Bool(_) => {}
    }
}

// This recursive impl is not on the deep-output path — output goes through the
// iterative `Value::format`. If a serde-based output path is ever added, give it a
// depth guard or an iterative serializer first; a captured-recursive query can nest
// values past the native stack.
impl Serialize for Value<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Value::Null => serializer.serialize_none(),
            Value::Node(h) => h.serialize(serializer),
            Value::Text(value) => serializer.serialize_str(value),
            Value::Bool(value) => serializer.serialize_bool(*value),
            Value::List(items) => {
                let mut seq = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            Value::Record(fields) => {
                let mut map = serializer.serialize_map(Some(fields.len()))?;
                for (key, value) in fields {
                    map.serialize_entry(key, value)?;
                }
                map.end()
            }
            Value::Variant { tag, data } => {
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

impl Value<'_> {
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

/// `indent` varies by recursion depth; the rest is shared state threaded through every call.
struct FormatCtx<'a> {
    out: &'a mut String,
    colors: &'a Colors,
    pretty: bool,
}

/// One emission for the iterative formatter's work stack.
enum WorkItem<'a> {
    /// Render a value: a leaf writes directly, a composite pushes its expansion.
    Value(&'a Value<'a>, usize),
    /// Write a borrowed slice verbatim. Color codes are `'static`; record keys and
    /// variant tags borrow the value.
    Str(&'a str),
    /// Write a record field key `"key":` (colored, escaped, trailing space in pretty).
    Key(&'a str),
    /// Pretty-mode line break followed by `n` indent spaces.
    Line(usize),
}

/// Format a value as colored JSON without native recursion.
///
/// Output depth tracks source depth for captured-recursive queries, and a high or
/// `Unbounded` depth limit lets it exceed any native-stack budget — so the walk
/// uses an explicit work stack. Emission is byte-identical to the equivalent
/// recursive printer; the `06-vm` golden fixtures pin that.
fn format_value<'a>(ctx: &mut FormatCtx<'_>, value: &'a Value<'a>, indent: usize) {
    let mut stack = vec![WorkItem::Value(value, indent)];
    while let Some(item) = stack.pop() {
        match item {
            WorkItem::Str(s) => ctx.out.push_str(s),
            WorkItem::Line(n) => {
                ctx.out.push('\n');
                push_indent(ctx.out, n);
            }
            WorkItem::Key(key) => emit_key(ctx, key),
            WorkItem::Value(value, indent) => emit_value(ctx, value, indent, &mut stack),
        }
    }
}

fn format_node_handle(ctx: &mut FormatCtx<'_>, h: &NodeHandle<'_>, indent: usize) {
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
    escape_json_into(out, h.text);
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

/// Write a value's own tokens, deferring any nested values onto `stack`. A leaf
/// writes fully; a composite writes its opener and queues children + closer so the
/// nesting is driven by the stack rather than the native call stack.
fn emit_value<'a>(
    ctx: &mut FormatCtx<'_>,
    value: &'a Value<'a>,
    indent: usize,
    stack: &mut Vec<WorkItem<'a>>,
) {
    let c = ctx.colors;
    match value {
        Value::Null => {
            ctx.out.push_str(c.dim);
            ctx.out.push_str("null");
            ctx.out.push_str(c.reset);
        }
        Value::Node(h) => format_node_handle(ctx, h, indent),
        Value::Text(value) => {
            ctx.out.push_str(c.green);
            ctx.out.push('"');
            escape_json_into(ctx.out, value);
            ctx.out.push('"');
            ctx.out.push_str(c.reset);
        }
        Value::Bool(value) => ctx.out.push_str(if *value { "true" } else { "false" }),
        Value::List(items) => {
            ctx.out.push_str(c.dim);
            ctx.out.push('[');
            ctx.out.push_str(c.reset);
            if items.is_empty() {
                ctx.out.push_str(c.dim);
                ctx.out.push(']');
                ctx.out.push_str(c.reset);
                return;
            }
            let elem_indent = if ctx.pretty { indent + 2 } else { 0 };
            let mut deferred = Vec::with_capacity(items.len() * 3 + 3);
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    deferred.push(WorkItem::Str(c.dim));
                    deferred.push(WorkItem::Str(","));
                    deferred.push(WorkItem::Str(c.reset));
                }
                if ctx.pretty {
                    deferred.push(WorkItem::Line(elem_indent));
                }
                deferred.push(WorkItem::Value(item, elem_indent));
            }
            if ctx.pretty {
                deferred.push(WorkItem::Line(indent));
            }
            deferred.push(WorkItem::Str(c.dim));
            deferred.push(WorkItem::Str("]"));
            deferred.push(WorkItem::Str(c.reset));
            stack.extend(deferred.into_iter().rev());
        }
        Value::Record(fields) => {
            ctx.out.push_str(c.dim);
            ctx.out.push('{');
            ctx.out.push_str(c.reset);
            if fields.is_empty() {
                ctx.out.push_str(c.dim);
                ctx.out.push('}');
                ctx.out.push_str(c.reset);
                return;
            }
            let field_indent = if ctx.pretty { indent + 2 } else { 0 };
            let mut deferred = Vec::with_capacity(fields.len() * 4 + 3);
            for (i, (key, value)) in fields.iter().enumerate() {
                if i > 0 {
                    deferred.push(WorkItem::Str(c.dim));
                    deferred.push(WorkItem::Str(","));
                    deferred.push(WorkItem::Str(c.reset));
                }
                if ctx.pretty {
                    deferred.push(WorkItem::Line(field_indent));
                }
                deferred.push(WorkItem::Key(key));
                deferred.push(WorkItem::Value(value, field_indent));
            }
            if ctx.pretty {
                deferred.push(WorkItem::Line(indent));
            }
            deferred.push(WorkItem::Str(c.dim));
            deferred.push(WorkItem::Str("}"));
            deferred.push(WorkItem::Str(c.reset));
            stack.extend(deferred.into_iter().rev());
        }
        Value::Variant { tag, data } => {
            ctx.out.push_str(c.dim);
            ctx.out.push('{');
            ctx.out.push_str(c.reset);
            let field_indent = if ctx.pretty { indent + 2 } else { 0 };
            if ctx.pretty {
                ctx.out.push('\n');
                push_indent(ctx.out, field_indent);
            }
            ctx.out.push_str(c.blue);
            ctx.out.push_str("\"$tag\"");
            ctx.out.push_str(c.reset);
            ctx.out.push_str(c.dim);
            ctx.out.push(':');
            ctx.out.push_str(c.reset);
            if ctx.pretty {
                ctx.out.push(' ');
            }
            ctx.out.push_str(c.green);
            ctx.out.push('"');
            escape_json_into(ctx.out, tag);
            ctx.out.push('"');
            ctx.out.push_str(c.reset);

            // Only the `$data` payload nests; the tag above is a leaf written in full.
            let mut deferred = Vec::new();
            if let Some(d) = data.as_deref() {
                ctx.out.push_str(c.dim);
                ctx.out.push(',');
                ctx.out.push_str(c.reset);
                if ctx.pretty {
                    ctx.out.push('\n');
                    push_indent(ctx.out, field_indent);
                }
                ctx.out.push_str(c.blue);
                ctx.out.push_str("\"$data\"");
                ctx.out.push_str(c.reset);
                ctx.out.push_str(c.dim);
                ctx.out.push(':');
                ctx.out.push_str(c.reset);
                if ctx.pretty {
                    ctx.out.push(' ');
                }
                deferred.push(WorkItem::Value(d, field_indent));
            }
            if ctx.pretty {
                deferred.push(WorkItem::Line(indent));
            }
            deferred.push(WorkItem::Str(c.dim));
            deferred.push(WorkItem::Str("}"));
            deferred.push(WorkItem::Str(c.reset));
            stack.extend(deferred.into_iter().rev());
        }
    }
}

/// Write a record field key `"key":` (colored, escaped, trailing space in pretty).
fn emit_key(ctx: &mut FormatCtx<'_>, key: &str) {
    let c = ctx.colors;
    ctx.out.push_str(c.blue);
    ctx.out.push('"');
    escape_json_into(ctx.out, key);
    ctx.out.push('"');
    ctx.out.push_str(c.reset);
    ctx.out.push_str(c.dim);
    ctx.out.push(':');
    ctx.out.push_str(c.reset);
    if ctx.pretty {
        ctx.out.push(' ');
    }
}

/// Escape `s` as a JSON string body, appending to `out`.
fn push_indent(out: &mut String, n: usize) {
    for _ in 0..n {
        out.push(' ');
    }
}
