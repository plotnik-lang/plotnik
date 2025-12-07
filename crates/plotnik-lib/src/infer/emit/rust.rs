//! Rust code emitter for inferred types.
//!
//! Emits Rust struct and enum definitions from a `TypeTable`.

use indexmap::IndexMap;

use super::super::types::{TypeKey, TypeTable, TypeValue};

/// Configuration for Rust emission.
#[derive(Debug, Clone)]
pub struct RustEmitConfig {
    /// Indirection type for cyclic references.
    pub indirection: Indirection,
    /// Whether to derive common traits.
    pub derive_debug: bool,
    pub derive_clone: bool,
    pub derive_partial_eq: bool,
    /// Name for the default (unnamed) query entry point type.
    pub default_query_name: String,
}

/// How to handle cyclic type references.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Indirection {
    Box,
    Rc,
    Arc,
}

impl Default for RustEmitConfig {
    fn default() -> Self {
        Self {
            indirection: Indirection::Box,
            derive_debug: true,
            derive_clone: true,
            derive_partial_eq: false,
            default_query_name: "QueryResult".to_string(),
        }
    }
}

/// Emit Rust code from a type table.
pub fn emit_rust(table: &TypeTable<'_>, config: &RustEmitConfig) -> String {
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
    config: &RustEmitConfig,
) -> String {
    let name = match key {
        TypeKey::DefaultQuery => config.default_query_name.clone(),
        _ => key.to_pascal_case(),
    };

    match value {
        TypeValue::Node | TypeValue::String | TypeValue::Unit | TypeValue::Invalid => String::new(),

        TypeValue::Struct(fields) => {
            let mut out = emit_derives(config);
            if fields.is_empty() {
                out.push_str(&format!("pub struct {};", name));
            } else {
                out.push_str(&format!("pub struct {} {{\n", name));
                for (field_name, field_type) in fields {
                    let type_str = emit_type_ref(field_type, table, config);
                    out.push_str(&format!("    pub {}: {},\n", field_name, type_str));
                }
                out.push('}');
            }
            out
        }

        TypeValue::TaggedUnion(variants) => {
            let mut out = emit_derives(config);
            out.push_str(&format!("pub enum {} {{\n", name));
            for (variant_name, variant_key) in variants {
                let fields = match table.get(variant_key) {
                    Some(TypeValue::Struct(f)) => Some(f),
                    Some(TypeValue::Unit) | None => None,
                    _ => None,
                };
                match fields {
                    Some(f) if !f.is_empty() => {
                        out.push_str(&format!("    {} {{\n", variant_name));
                        for (field_name, field_type) in f {
                            let type_str = emit_type_ref(field_type, table, config);
                            out.push_str(&format!("        {}: {},\n", field_name, type_str));
                        }
                        out.push_str("    },\n");
                    }
                    _ => {
                        out.push_str(&format!("    {},\n", variant_name));
                    }
                }
            }
            out.push('}');
            out
        }

        TypeValue::Optional(_) | TypeValue::List(_) | TypeValue::NonEmptyList(_) => {
            // Wrapper types become type aliases
            let mut out = String::new();
            let inner_type = emit_type_ref(key, table, config);
            out.push_str(&format!("pub type {} = {};", name, inner_type));
            out
        }
    }
}

pub(crate) fn emit_type_ref(
    key: &TypeKey<'_>,
    table: &TypeTable<'_>,
    config: &RustEmitConfig,
) -> String {
    let is_cyclic = table.is_cyclic(key);

    let base = match table.get(key) {
        Some(TypeValue::Node) => "Node".to_string(),
        Some(TypeValue::String) => "String".to_string(),
        Some(TypeValue::Unit) | Some(TypeValue::Invalid) => "()".to_string(),
        Some(TypeValue::Optional(inner)) => {
            let inner_str = emit_type_ref(inner, table, config);
            format!("Option<{}>", inner_str)
        }
        Some(TypeValue::List(inner)) => {
            let inner_str = emit_type_ref(inner, table, config);
            format!("Vec<{}>", inner_str)
        }
        Some(TypeValue::NonEmptyList(inner)) => {
            let inner_str = emit_type_ref(inner, table, config);
            format!("Vec<{}>", inner_str)
        }
        // Struct, TaggedUnion, or undefined forward reference - use pascal-cased name
        Some(TypeValue::Struct(_)) | Some(TypeValue::TaggedUnion(_)) | None => match key {
            TypeKey::DefaultQuery => config.default_query_name.clone(),
            _ => key.to_pascal_case(),
        },
    };

    if is_cyclic {
        wrap_indirection(&base, config.indirection)
    } else {
        base
    }
}

pub(crate) fn wrap_indirection(type_str: &str, indirection: Indirection) -> String {
    match indirection {
        Indirection::Box => format!("Box<{}>", type_str),
        Indirection::Rc => format!("Rc<{}>", type_str),
        Indirection::Arc => format!("Arc<{}>", type_str),
    }
}

pub(crate) fn emit_derives(config: &RustEmitConfig) -> String {
    let mut derives = Vec::new();
    if config.derive_debug {
        derives.push("Debug");
    }
    if config.derive_clone {
        derives.push("Clone");
    }
    if config.derive_partial_eq {
        derives.push("PartialEq");
    }

    if derives.is_empty() {
        String::new()
    } else {
        format!("#[derive({})]\n", derives.join(", "))
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
