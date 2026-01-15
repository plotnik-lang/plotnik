//! Debug-only type verification for materialized values.
//!
//! Verifies that materialized `Value` matches the declared `result_type` from bytecode.
//! Zero-cost in release builds.

use plotnik_bytecode::{Module, TypeId};
#[cfg(debug_assertions)]
use plotnik_bytecode::{StringsView, TypeData, TypeKind, TypesView};
use plotnik_core::Colors;

use super::Value;

/// Debug-only type verification.
///
/// Panics with a pretty diagnostic if the value doesn't match the declared type.
/// This is a no-op in release builds.
///
/// `declared_type` should be the `result_type` from the entrypoint that was executed.
#[cfg(debug_assertions)]
pub fn debug_verify_type(value: &Value, declared_type: TypeId, module: &Module, colors: Colors) {
    let types = module.types();
    let strings = module.strings();

    let mut errors = Vec::new();
    verify_type(
        value,
        declared_type,
        &types,
        &strings,
        &mut String::new(),
        &mut errors,
    );
    if !errors.is_empty() {
        panic_with_mismatch(value, declared_type, &errors, module, colors);
    }
}

/// No-op in release builds.
#[cfg(not(debug_assertions))]
#[inline(always)]
pub fn debug_verify_type(
    _value: &Value,
    _declared_type: TypeId,
    _module: &Module,
    _colors: Colors,
) {
}

/// Recursive type verification. Collects mismatch paths into `errors`.
#[cfg(debug_assertions)]
fn verify_type(
    value: &Value,
    declared: TypeId,
    types: &TypesView<'_>,
    strings: &StringsView<'_>,
    path: &mut String,
    errors: &mut Vec<String>,
) {
    let Some(type_def) = types.get(declared) else {
        errors.push(format_error(
            path,
            &format!("unknown type id {}", declared.0),
        ));
        return;
    };

    match type_def.classify() {
        TypeData::Primitive(kind) => match kind {
            TypeKind::Void => {
                if !matches!(value, Value::Null) {
                    errors.push(format_error(
                        path,
                        &format!("type: void, value: {}", value_kind_name(value)),
                    ));
                }
            }
            TypeKind::Node => {
                if !matches!(value, Value::Node(_)) {
                    errors.push(format_error(
                        path,
                        &format!("type: Node, value: {}", value_kind_name(value)),
                    ));
                }
            }
            TypeKind::String => {
                if !matches!(value, Value::String(_)) {
                    errors.push(format_error(
                        path,
                        &format!("type: string, value: {}", value_kind_name(value)),
                    ));
                }
            }
            _ => unreachable!(),
        },

        TypeData::Wrapper { kind, inner } => match kind {
            TypeKind::Alias => {
                if !matches!(value, Value::Node(_)) {
                    errors.push(format_error(
                        path,
                        &format!("type: Node (alias), value: {}", value_kind_name(value)),
                    ));
                }
            }
            TypeKind::Optional => {
                if !matches!(value, Value::Null) {
                    verify_type(value, inner, types, strings, path, errors);
                }
            }
            TypeKind::ArrayZeroOrMore => match value {
                Value::Array(items) => {
                    for (i, item) in items.iter().enumerate() {
                        let prev_len = path.len();
                        path.push_str(&format!("[{}]", i));
                        verify_type(item, inner, types, strings, path, errors);
                        path.truncate(prev_len);
                    }
                }
                _ => {
                    errors.push(format_error(
                        path,
                        &format!("type: array, value: {}", value_kind_name(value)),
                    ));
                }
            },
            TypeKind::ArrayOneOrMore => match value {
                Value::Array(items) => {
                    if items.is_empty() {
                        errors.push(format_error(
                            path,
                            "type: non-empty array, value: empty array",
                        ));
                    }
                    for (i, item) in items.iter().enumerate() {
                        let prev_len = path.len();
                        path.push_str(&format!("[{}]", i));
                        verify_type(item, inner, types, strings, path, errors);
                        path.truncate(prev_len);
                    }
                }
                _ => {
                    errors.push(format_error(
                        path,
                        &format!("type: array, value: {}", value_kind_name(value)),
                    ));
                }
            },
            _ => unreachable!(),
        },

        TypeData::Composite { kind, .. } => match kind {
            TypeKind::Struct => match value {
                Value::Object(fields) => {
                    for member in types.members_of(&type_def) {
                        let field_name = strings.get(member.name);
                        let (inner_type, is_optional) = types.unwrap_optional(member.type_id);

                        let field_value = fields.iter().find(|(k, _)| k == field_name);
                        match field_value {
                            Some((_, v)) => {
                                if is_optional && matches!(v, Value::Null) {
                                    continue;
                                }
                                let prev_len = path.len();
                                path.push('.');
                                path.push_str(field_name);
                                verify_type(v, inner_type, types, strings, path, errors);
                                path.truncate(prev_len);
                            }
                            None => {
                                if !is_optional {
                                    errors.push(format!(
                                        "{}: required field missing",
                                        append_path(path, field_name)
                                    ));
                                }
                            }
                        }
                    }
                }
                _ => {
                    errors.push(format_error(
                        path,
                        &format!("type: object, value: {}", value_kind_name(value)),
                    ));
                }
            },
            TypeKind::Enum => match value {
                Value::Tagged { tag, data } => {
                    let variant = types
                        .members_of(&type_def)
                        .find(|m| strings.get(m.name) == tag);

                    match variant {
                        Some(member) => {
                            let is_void = types.get(member.type_id).is_some_and(|d| {
                                matches!(d.classify(), TypeData::Primitive(TypeKind::Void))
                            });

                            if is_void {
                                if data.is_some() {
                                    errors.push(format!(
                                        "{}: void variant '{}' should have no $data",
                                        append_path(path, "$data"),
                                        tag
                                    ));
                                }
                            } else {
                                match data {
                                    Some(d) => {
                                        let prev_len = path.len();
                                        path.push_str(".$data");
                                        verify_type(
                                            d,
                                            member.type_id,
                                            types,
                                            strings,
                                            path,
                                            errors,
                                        );
                                        path.truncate(prev_len);
                                    }
                                    None => {
                                        errors.push(format!(
                                            "{}: non-void variant '{}' should have $data",
                                            append_path(path, "$data"),
                                            tag
                                        ));
                                    }
                                }
                            }
                        }
                        None => {
                            errors.push(format!(
                                "{}: unknown variant '{}'",
                                append_path(path, "$tag"),
                                tag
                            ));
                        }
                    }
                }
                _ => {
                    errors.push(format_error(
                        path,
                        &format!("type: tagged union, value: {}", value_kind_name(value)),
                    ));
                }
            },
            _ => unreachable!(),
        },
    }
}

/// Get a display name for the value's kind.
#[cfg(debug_assertions)]
fn value_kind_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::String(_) => "string",
        Value::Node(_) => "Node",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
        Value::Tagged { .. } => "tagged union",
    }
}

/// Format path for error message. Leading dot is stripped.
#[cfg(debug_assertions)]
fn format_path(path: &str) -> String {
    path.strip_prefix('.').unwrap_or(path).to_string()
}

/// Format error with optional path prefix.
#[cfg(debug_assertions)]
fn format_error(path: &str, msg: &str) -> String {
    let p = format_path(path);
    if p.is_empty() {
        msg.to_string()
    } else {
        format!("{}: {}", p, msg)
    }
}

/// Append a suffix to a path, handling empty path case.
#[cfg(debug_assertions)]
fn append_path(path: &str, suffix: &str) -> String {
    let p = format_path(path);
    if p.is_empty() {
        suffix.to_string()
    } else {
        format!("{}.{}", p, suffix)
    }
}

/// Create a centered header line with dashes.
#[cfg(debug_assertions)]
fn centered_header(label: &str, width: usize) -> String {
    let label_with_spaces = format!(" {} ", label);
    let label_len = label_with_spaces.len();
    if label_len >= width {
        return label_with_spaces;
    }
    let remaining = width - label_len;
    let left = remaining / 2;
    let right = remaining - left;
    format!(
        "{}{}{}",
        "-".repeat(left),
        label_with_spaces,
        "-".repeat(right)
    )
}

/// Panic with a pretty diagnostic showing the type mismatch.
#[cfg(debug_assertions)]
fn panic_with_mismatch(
    value: &Value,
    declared_type: TypeId,
    errors: &[String],
    module: &Module,
    colors: Colors,
) -> ! {
    const WIDTH: usize = 80;
    let separator = "=".repeat(WIDTH);

    let entrypoints = module.entrypoints();
    let strings = module.strings();

    // Find the entrypoint name by matching result_type
    let type_name = (0..entrypoints.len())
        .find_map(|i| {
            let e = entrypoints.get(i);
            if e.result_type() == declared_type {
                Some(strings.get(e.name()))
            } else {
                None
            }
        })
        .unwrap_or("unknown");

    let value_str = value.format(true, colors);
    let details_str = errors.join("\n");

    let output_header = centered_header(&format!("Output: {}", type_name), WIDTH);
    let details_header = centered_header("Details", WIDTH);

    panic!(
        "\n{separator}\n\
         BUG: Type and value do not match\n\
         {separator}\n\n\
         {output_header}\n\n\
         {value_str}\n\n\
         {details_header}\n\n\
         {details_str}\n\n\
         {separator}\n"
    );
}
