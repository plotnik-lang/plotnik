//! Debug-only JSON rendering for generated query results.

use std::fmt::Write as _;

use serde::Serialize;
use serde_json::{Map, Value};

/// Serialize a generated result using Plotnik's canonical debug layout.
pub fn to_json<T>(value: &T) -> Result<String, serde_json::Error>
where
    T: Serialize + ?Sized,
{
    let mut value = serde_json::to_value(value)?;
    value.sort_all_objects();

    let mut out = String::new();
    format_value(&mut out, &value, 0, false);
    Ok(out)
}

fn format_value(out: &mut String, value: &Value, indent: usize, compact: bool) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        Value::Number(value) => write!(out, "{value}").expect("writing to a String is infallible"),
        Value::String(value) => out.push_str(
            &serde_json::to_string(value).expect("serializing a JSON string is infallible"),
        ),
        Value::Array(values) => format_array(out, values, indent, compact),
        Value::Object(fields) => format_object(out, fields, indent),
    }
}

fn format_array(out: &mut String, values: &[Value], indent: usize, compact: bool) {
    if values.is_empty() {
        out.push_str("[]");
        return;
    }
    if compact {
        out.push('[');
        format_value(out, &values[0], indent, false);
        out.push_str(", ");
        format_value(out, &values[1], indent, false);
        out.push(']');
        return;
    }

    out.push_str("[\n");
    for (index, value) in values.iter().enumerate() {
        push_indent(out, indent + 2);
        format_value(out, value, indent + 2, false);
        if index + 1 < values.len() {
            out.push(',');
        }
        out.push('\n');
    }
    push_indent(out, indent);
    out.push(']');
}

fn format_object(out: &mut String, fields: &Map<String, Value>, indent: usize) {
    if fields.is_empty() {
        out.push_str("{}");
        return;
    }

    let node = is_node(fields);
    out.push_str("{\n");
    for (index, (key, value)) in fields.iter().enumerate() {
        push_indent(out, indent + 2);
        out.push_str(&serde_json::to_string(key).expect("serializing a JSON key is infallible"));
        out.push_str(": ");
        format_value(out, value, indent + 2, node && key == "span");
        if index + 1 < fields.len() {
            out.push(',');
        }
        out.push('\n');
    }
    push_indent(out, indent);
    out.push('}');
}

fn is_node(fields: &Map<String, Value>) -> bool {
    if fields.len() != 3
        || !fields.get("kind").is_some_and(Value::is_string)
        || !fields.get("text").is_some_and(Value::is_string)
    {
        return false;
    }
    let Some(Value::Array(span)) = fields.get("span") else {
        return false;
    };
    span.len() == 2 && span.iter().all(|value| value.as_u64().is_some())
}

fn push_indent(out: &mut String, indent: usize) {
    out.extend(std::iter::repeat_n(' ', indent));
}
