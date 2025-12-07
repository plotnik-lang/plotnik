//! TypeScript code emitter for inferred types.
//!
//! Emits TypeScript interface and type definitions from a `TypeTable`.

use indexmap::IndexMap;

use super::super::types::{TypeKey, TypeTable, TypeValue};

/// Configuration for TypeScript emission.
#[derive(Debug, Clone)]
pub struct TypeScriptEmitConfig {
    /// How to represent optional values.
    pub optional: OptionalStyle,
    /// Whether to export types.
    pub export: bool,
    /// Whether to make fields readonly.
    pub readonly: bool,
    /// Whether to emit nested synthetic types instead of inlining them.
    pub nested: bool,
    /// Name for the Node type.
    pub node_type: String,
    /// Whether to emit `type Foo = ...` instead of `interface Foo { ... }`.
    pub type_alias: bool,
    /// Name for the default (unnamed) query entry point.
    pub entry_name: String,
}

/// How to represent optional types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionalStyle {
    /// `T | null`
    Null,
    /// `T | undefined`
    Undefined,
    /// `T?` (optional property)
    QuestionMark,
}

impl Default for TypeScriptEmitConfig {
    fn default() -> Self {
        Self {
            optional: OptionalStyle::Null,
            export: false,
            readonly: false,
            nested: false,
            node_type: "SyntaxNode".to_string(),
            type_alias: false,
            entry_name: "QueryResult".to_string(),
        }
    }
}

/// Emit TypeScript code from a type table.
pub fn emit_typescript(table: &TypeTable<'_>, config: &TypeScriptEmitConfig) -> String {
    let mut output = String::new();
    let sorted = topological_sort(table);

    for key in sorted {
        let Some(value) = table.get(&key) else {
            continue;
        };

        // Skip built-in types
        if matches!(key, TypeKey::Node | TypeKey::String | TypeKey::Unit) {
            continue;
        }

        // Skip synthetic types if not nested (i.e., inlining)
        if !config.nested && matches!(key, TypeKey::Synthetic { .. }) {
            continue;
        }

        let type_def = emit_type_def(&key, value, table, config);
        if !type_def.is_empty() {
            output.push_str(&type_def);
            output.push_str("\n\n");
        }
    }

    output.trim_end().to_string()
}

fn emit_type_def(
    key: &TypeKey<'_>,
    value: &TypeValue<'_>,
    table: &TypeTable<'_>,
    config: &TypeScriptEmitConfig,
) -> String {
    let name = type_name(key, config);
    let export_prefix = if config.export && !matches!(key, TypeKey::Synthetic { .. }) {
        "export "
    } else {
        ""
    };

    match value {
        TypeValue::Node | TypeValue::String | TypeValue::Unit | TypeValue::Invalid => String::new(),

        TypeValue::Struct(fields) => {
            if config.type_alias {
                let inline = emit_inline_struct(fields, table, config);
                format!("{}type {} = {};", export_prefix, name, inline)
            } else if fields.is_empty() {
                format!("{}interface {} {{}}", export_prefix, name)
            } else {
                let mut out = format!("{}interface {} {{\n", export_prefix, name);
                for (field_name, field_type) in fields {
                    let (type_str, is_optional) = emit_field_type(field_type, table, config);
                    let readonly = if config.readonly { "readonly " } else { "" };
                    let optional = if is_optional && config.optional == OptionalStyle::QuestionMark
                    {
                        "?"
                    } else {
                        ""
                    };
                    out.push_str(&format!(
                        "  {}{}{}: {};\n",
                        readonly, field_name, optional, type_str
                    ));
                }
                out.push('}');
                out
            }
        }

        TypeValue::TaggedUnion(variants) => {
            let mut out = format!("{}type {} =\n", export_prefix, name);
            let variant_count = variants.len();
            for (i, (variant_name, variant_key)) in variants.iter().enumerate() {
                out.push_str("  | { tag: \"");
                out.push_str(variant_name);
                out.push('"');
                // Look up variant type to get fields
                if let Some(TypeValue::Struct(fields)) = table.get(variant_key) {
                    for (field_name, field_type) in fields {
                        let (type_str, is_optional) = emit_field_type(field_type, table, config);
                        let optional =
                            if is_optional && config.optional == OptionalStyle::QuestionMark {
                                "?"
                            } else {
                                ""
                            };
                        out.push_str(&format!("; {}{}: {}", field_name, optional, type_str));
                    }
                }
                out.push_str(" }");
                if i < variant_count - 1 {
                    out.push('\n');
                }
            }
            out.push(';');
            out
        }

        TypeValue::Optional(_) | TypeValue::List(_) | TypeValue::NonEmptyList(_) => {
            let (type_str, _) = emit_field_type(key, table, config);
            format!("{}type {} = {};", export_prefix, name, type_str)
        }
    }
}

/// Returns (type_string, is_optional)
pub(crate) fn emit_field_type(
    key: &TypeKey<'_>,
    table: &TypeTable<'_>,
    config: &TypeScriptEmitConfig,
) -> (String, bool) {
    match table.get(key) {
        Some(TypeValue::Node) => (config.node_type.clone(), false),
        Some(TypeValue::String) => ("string".to_string(), false),
        Some(TypeValue::Unit) | Some(TypeValue::Invalid) => ("{}".to_string(), false),

        Some(TypeValue::Optional(inner)) => {
            let (inner_str, _) = emit_field_type(inner, table, config);
            let type_str = match config.optional {
                OptionalStyle::Null => format!("{} | null", inner_str),
                OptionalStyle::Undefined => format!("{} | undefined", inner_str),
                OptionalStyle::QuestionMark => inner_str,
            };
            (type_str, true)
        }

        Some(TypeValue::List(inner)) => {
            let (inner_str, _) = emit_field_type(inner, table, config);
            (format!("{}[]", wrap_if_union(&inner_str)), false)
        }

        Some(TypeValue::NonEmptyList(inner)) => {
            let (inner_str, _) = emit_field_type(inner, table, config);
            (format!("[{}, ...{}[]]", inner_str, inner_str), false)
        }

        Some(TypeValue::Struct(fields)) => {
            if !config.nested && matches!(key, TypeKey::Synthetic { .. }) {
                (emit_inline_struct(fields, table, config), false)
            } else {
                (type_name(key, config), false)
            }
        }

        Some(TypeValue::TaggedUnion(_)) => (type_name(key, config), false),

        None => (type_name(key, config), false),
    }
}

pub(crate) fn emit_inline_struct(
    fields: &IndexMap<&str, TypeKey<'_>>,
    table: &TypeTable<'_>,
    config: &TypeScriptEmitConfig,
) -> String {
    if fields.is_empty() {
        return "{}".to_string();
    }

    let mut out = String::from("{ ");
    for (i, (field_name, field_type)) in fields.iter().enumerate() {
        let (type_str, is_optional) = emit_field_type(field_type, table, config);
        let optional = if is_optional && config.optional == OptionalStyle::QuestionMark {
            "?"
        } else {
            ""
        };
        out.push_str(field_name);
        out.push_str(optional);
        out.push_str(": ");
        out.push_str(&type_str);
        if i < fields.len() - 1 {
            out.push_str("; ");
        }
    }
    out.push_str(" }");
    out
}

fn type_name(key: &TypeKey<'_>, config: &TypeScriptEmitConfig) -> String {
    key.to_pascal_case_with_entry_name(&config.entry_name)
}

pub(crate) fn wrap_if_union(type_str: &str) -> String {
    if type_str.contains('|') {
        format!("({})", type_str)
    } else {
        type_str.to_string()
    }
}

/// Topologically sort types so dependencies come before dependents.
pub(crate) fn topological_sort<'src>(table: &TypeTable<'src>) -> Vec<TypeKey<'src>> {
    let mut result = Vec::new();
    let mut visited = IndexMap::new();

    for key in table.types.keys() {
        visit(key, table, &mut visited, &mut result);
    }

    result
}

fn visit<'src>(
    key: &TypeKey<'src>,
    table: &TypeTable<'src>,
    visited: &mut IndexMap<TypeKey<'src>, bool>,
    result: &mut Vec<TypeKey<'src>>,
) {
    if visited.contains_key(key) {
        return;
    }

    visited.insert(key.clone(), true);

    let Some(value) = table.get(key) else {
        visited.insert(key.clone(), false);
        result.push(key.clone());
        return;
    };

    for dep in dependencies(value) {
        visit(&dep, table, visited, result);
    }

    visited.insert(key.clone(), false);
    result.push(key.clone());
}

pub(crate) fn dependencies<'src>(value: &TypeValue<'src>) -> Vec<TypeKey<'src>> {
    match value {
        TypeValue::Node | TypeValue::String | TypeValue::Unit | TypeValue::Invalid => vec![],
        TypeValue::Struct(fields) => fields.values().cloned().collect(),
        TypeValue::TaggedUnion(variants) => variants.values().cloned().collect(),
        TypeValue::Optional(inner) | TypeValue::List(inner) | TypeValue::NonEmptyList(inner) => {
            vec![inner.clone()]
        }
    }
}
