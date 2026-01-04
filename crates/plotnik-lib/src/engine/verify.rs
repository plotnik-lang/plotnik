//! Debug-only type verification for materialized values.
//!
//! Verifies that materialized `Value` matches the declared `result_type` from bytecode.
//! Zero-cost in release builds.

use crate::bytecode::{Module, QTypeId, StringsView, TypesView};
use crate::type_system::TypeKind;
use crate::typegen::typescript::{self, Config, VoidType};
use crate::Colors;

use super::Value;

/// Debug-only type verification.
///
/// Panics with a pretty diagnostic if the value doesn't match the expected type.
/// This is a no-op in release builds.
#[cfg(debug_assertions)]
pub fn debug_verify_type(value: &Value, module: &Module, colors: Colors) {
    let types = module.types();
    let strings = module.strings();
    let entrypoints = module.entrypoints();

    // Get the first entrypoint's result type for verification
    if entrypoints.is_empty() {
        return;
    }
    let entrypoint = entrypoints.get(0);
    let expected_type = entrypoint.result_type;

    let mut errors = Vec::new();
    verify_type(
        value,
        expected_type,
        &types,
        &strings,
        &mut String::new(),
        &mut errors,
    );
    if !errors.is_empty() {
        panic_with_mismatch(value, &errors, module, colors);
    }
}

/// No-op in release builds.
#[cfg(not(debug_assertions))]
#[inline(always)]
pub fn debug_verify_type(_value: &Value, _module: &Module, _colors: Colors) {}

/// Recursive type verification. Collects mismatch paths into `errors`.
#[cfg(debug_assertions)]
fn verify_type(
    value: &Value,
    expected: QTypeId,
    types: &TypesView<'_>,
    strings: &StringsView<'_>,
    path: &mut String,
    errors: &mut Vec<String>,
) {
    let Some(type_def) = types.get(expected) else {
        errors.push(format_error(
            path,
            &format!("unknown type id {}", expected.0),
        ));
        return;
    };

    let Some(kind) = type_def.type_kind() else {
        errors.push(format_error(path, "invalid type kind"));
        return;
    };

    match kind {
        TypeKind::Void => {
            if !matches!(value, Value::Null) {
                errors.push(format_error(
                    path,
                    &format!("expected void (null), found {}", value_kind_name(value)),
                ));
            }
        }

        TypeKind::Node => {
            if !matches!(value, Value::Node(_)) {
                errors.push(format_error(
                    path,
                    &format!("expected Node, found {}", value_kind_name(value)),
                ));
            }
        }

        TypeKind::String => {
            if !matches!(value, Value::String(_)) {
                errors.push(format_error(
                    path,
                    &format!("expected string, found {}", value_kind_name(value)),
                ));
            }
        }

        TypeKind::Alias => {
            if !matches!(value, Value::Node(_)) {
                errors.push(format_error(
                    path,
                    &format!("expected Node (alias), found {}", value_kind_name(value)),
                ));
            }
        }

        TypeKind::Optional => {
            let inner_type = QTypeId(type_def.data);
            if !matches!(value, Value::Null) {
                verify_type(value, inner_type, types, strings, path, errors);
            }
        }

        TypeKind::ArrayZeroOrMore => {
            let inner_type = QTypeId(type_def.data);
            match value {
                Value::Array(items) => {
                    for (i, item) in items.iter().enumerate() {
                        let prev_len = path.len();
                        path.push_str(&format!("[{}]", i));
                        verify_type(item, inner_type, types, strings, path, errors);
                        path.truncate(prev_len);
                    }
                }
                _ => {
                    errors.push(format_error(
                        path,
                        &format!("expected array, found {}", value_kind_name(value)),
                    ));
                }
            }
        }

        TypeKind::ArrayOneOrMore => {
            let inner_type = QTypeId(type_def.data);
            match value {
                Value::Array(items) => {
                    if items.is_empty() {
                        errors.push(format_error(
                            path,
                            "expected non-empty array, found empty array",
                        ));
                    }
                    for (i, item) in items.iter().enumerate() {
                        let prev_len = path.len();
                        path.push_str(&format!("[{}]", i));
                        verify_type(item, inner_type, types, strings, path, errors);
                        path.truncate(prev_len);
                    }
                }
                _ => {
                    errors.push(format_error(
                        path,
                        &format!("expected array, found {}", value_kind_name(value)),
                    ));
                }
            }
        }

        TypeKind::Struct => match value {
            Value::Object(fields) => {
                for member in types.members_of(&type_def) {
                    let field_name = strings.get(member.name);
                    let (inner_type, is_optional) = types.unwrap_optional(member.type_id);

                    let field_value = fields.iter().find(|(k, _)| k == field_name);
                    match field_value {
                        Some((_, v)) => {
                            if is_optional && matches!(v, Value::Null) {
                                continue; // null is valid for optional field
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
                    &format!("expected object, found {}", value_kind_name(value)),
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
                        let is_void = types
                            .get(member.type_id)
                            .and_then(|d| d.type_kind())
                            .is_some_and(|k| k == TypeKind::Void);

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
                                    verify_type(d, member.type_id, types, strings, path, errors);
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
                    &format!("expected tagged union, found {}", value_kind_name(value)),
                ));
            }
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

/// Create a centered header line with dashes, e.g. "--- Label ---" -> "--------------------------------- Label ----------------------------------"
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
fn panic_with_mismatch(value: &Value, errors: &[String], module: &Module, colors: Colors) -> ! {
    const WIDTH: usize = 80;
    let separator = "=".repeat(WIDTH);

    let entrypoints = module.entrypoints();
    let strings = module.strings();
    let type_name = if !entrypoints.is_empty() {
        strings.get(entrypoints.get(0).name)
    } else {
        "unknown"
    };

    let config = Config {
        export: true,
        emit_node_type: true,
        verbose_nodes: false,
        void_type: VoidType::Null,
        colors: Colors::OFF,
    };
    let type_str = typescript::emit_with_config(module, config);
    let value_str = value.format(true, colors);
    let details_str = errors.join("\n");

    let output_header = centered_header(&format!("Output: {}", type_name), WIDTH);
    let details_header = centered_header("Details", WIDTH);

    panic!(
        "\n{separator}\n\
         TYPE MISMATCH: Query output does not match declared type\n\
         {separator}\n\n\
         {type_str}\n\
         {output_header}\n\n\
         {value_str}\n\n\
         {details_header}\n\n\
         {details_str}\n\n\
         {separator}\n"
    );
}

#[cfg(test)]
#[path = "verify_tests.rs"]
mod verify_tests;
