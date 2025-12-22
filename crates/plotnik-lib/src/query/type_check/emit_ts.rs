//! TypeScript type emitter for testing type inference.
//!
//! Converts inferred types to TypeScript declarations.
//! Used as a test oracle to verify type inference correctness.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::context::TypeContext;
use super::types::{FieldInfo, TYPE_NODE, TYPE_STRING, TYPE_VOID, TypeId, TypeKind};

/// Configuration for TypeScript emission.
#[derive(Clone, Debug)]
pub struct EmitConfig {
    /// Whether to export types
    pub export: bool,
    /// Whether to emit the Node type definition
    pub emit_node_type: bool,
    /// Name for the root type if unnamed
    pub root_type_name: String,
    /// Use verbose node representation (with kind, text, etc.)
    pub verbose_nodes: bool,
}

impl Default for EmitConfig {
    fn default() -> Self {
        Self {
            export: true,
            emit_node_type: true,
            root_type_name: "Query".to_string(),
            verbose_nodes: false,
        }
    }
}

/// TypeScript emitter.
pub struct TsEmitter<'a> {
    ctx: &'a TypeContext,
    config: EmitConfig,
    /// Generated type names, to avoid collisions
    used_names: BTreeSet<String>,
    /// TypeId -> generated name mapping
    type_names: HashMap<TypeId, String>,
    /// Output buffer
    output: String,
}

impl<'a> TsEmitter<'a> {
    pub fn new(ctx: &'a TypeContext, config: EmitConfig) -> Self {
        Self {
            ctx,
            config,
            used_names: BTreeSet::new(),
            type_names: HashMap::new(),
            output: String::new(),
        }
    }

    /// Emit TypeScript for all definition types.
    pub fn emit(mut self) -> String {
        // First pass: collect all types that need names
        self.collect_type_names();

        // Emit Node type if configured
        if self.config.emit_node_type {
            self.emit_node_type();
        }

        // Emit each definition type
        for (name, type_id) in self.ctx.iter_def_types() {
            self.emit_definition(name, type_id);
        }

        self.output
    }

    /// Emit TypeScript for a single definition.
    pub fn emit_single(mut self, name: &str, type_id: TypeId) -> String {
        self.collect_type_names();

        if self.config.emit_node_type {
            self.emit_node_type();
        }

        self.emit_definition(name, type_id);
        self.output
    }

    fn collect_type_names(&mut self) {
        // Reserve definition names first
        for (name, _) in self.ctx.iter_def_types() {
            let pascal_name = to_pascal_case(name);
            self.used_names.insert(pascal_name);
        }

        // Then assign names to anonymous types
        for (id, kind) in self.ctx.iter_types() {
            if self.needs_named_type(kind) && !self.type_names.contains_key(&id) {
                let name = self.generate_type_name(kind);
                self.type_names.insert(id, name);
            }
        }
    }

    fn needs_named_type(&self, kind: &TypeKind) -> bool {
        matches!(kind, TypeKind::Struct(_) | TypeKind::Enum(_))
    }

    fn generate_type_name(&mut self, kind: &TypeKind) -> String {
        let base = match kind {
            TypeKind::Struct(_) => "Struct",
            TypeKind::Enum(_) => "Enum",
            _ => "Type",
        };

        self.unique_name(base)
    }

    fn unique_name(&mut self, base: &str) -> String {
        let base = to_pascal_case(base);
        if !self.used_names.contains(&base) {
            self.used_names.insert(base.clone());
            return base;
        }

        let mut counter = 2;
        loop {
            let name = format!("{}{}", base, counter);
            if !self.used_names.contains(&name) {
                self.used_names.insert(name.clone());
                return name;
            }
            counter += 1;
        }
    }

    fn emit_node_type(&mut self) {
        let export = if self.config.export { "export " } else { "" };

        if self.config.verbose_nodes {
            self.output.push_str(&format!(
                "{}interface Node {{\n  kind: string;\n  text: string;\n  startPosition: {{ row: number; column: number }};\n  endPosition: {{ row: number; column: number }};\n}}\n\n",
                export
            ));
        } else {
            self.output.push_str(&format!(
                "{}interface Node {{\n  kind: string;\n  text: string;\n}}\n\n",
                export
            ));
        }
    }

    fn emit_definition(&mut self, name: &str, type_id: TypeId) {
        let export = if self.config.export { "export " } else { "" };
        let type_name = to_pascal_case(name);

        let Some(kind) = self.ctx.get_type(type_id) else {
            return;
        };

        match kind {
            TypeKind::Struct(fields) => {
                self.emit_interface(&type_name, fields, export);
            }
            TypeKind::Enum(variants) => {
                self.emit_tagged_union(&type_name, variants, export);
            }
            _ => {
                // For non-struct types, emit a type alias
                let ts_type = self.type_to_ts(type_id);
                self.output
                    .push_str(&format!("{}type {} = {};\n\n", export, type_name, ts_type));
            }
        }
    }

    fn emit_interface(&mut self, name: &str, fields: &BTreeMap<String, FieldInfo>, export: &str) {
        self.output
            .push_str(&format!("{}interface {} {{\n", export, name));

        for (field_name, info) in fields {
            let ts_type = self.type_to_ts(info.type_id);
            let optional = if info.optional { "?" } else { "" };
            self.output
                .push_str(&format!("  {}{}: {};\n", field_name, optional, ts_type));
        }

        self.output.push_str("}\n\n");

        // Emit nested types
        for (_, info) in fields {
            self.maybe_emit_nested_type(info.type_id);
        }
    }

    fn emit_tagged_union(&mut self, name: &str, variants: &BTreeMap<String, TypeId>, export: &str) {
        // Emit variant types first
        let mut variant_types = Vec::new();
        for (variant_name, type_id) in variants {
            let variant_type_name = format!("{}{}", name, to_pascal_case(variant_name));
            variant_types.push(variant_type_name.clone());

            let data_type = self.type_to_ts(*type_id);
            self.output.push_str(&format!(
                "{}interface {} {{\n  $tag: \"{}\";\n  $data: {};\n}}\n\n",
                export, variant_type_name, variant_name, data_type
            ));

            self.maybe_emit_nested_type(*type_id);
        }

        // Emit union type
        let union = variant_types.join(" | ");
        self.output
            .push_str(&format!("{}type {} = {};\n\n", export, name, union));
    }

    fn maybe_emit_nested_type(&mut self, type_id: TypeId) {
        let Some(kind) = self.ctx.get_type(type_id) else {
            return;
        };

        // Skip if already emitted or is a primitive
        if type_id.is_builtin() {
            return;
        }

        match kind {
            TypeKind::Struct(fields) => {
                if let Some(name) = self.type_names.get(&type_id) {
                    let name = name.clone();
                    let export = if self.config.export { "export " } else { "" };
                    self.emit_interface(&name, fields, export);
                }
            }
            TypeKind::Array { element, .. } => {
                self.maybe_emit_nested_type(*element);
            }
            TypeKind::Optional(inner) => {
                self.maybe_emit_nested_type(*inner);
            }
            _ => {}
        }
    }

    fn type_to_ts(&self, type_id: TypeId) -> String {
        if type_id == TYPE_VOID {
            return "void".to_string();
        }
        if type_id == TYPE_NODE {
            return "Node".to_string();
        }
        if type_id == TYPE_STRING {
            return "string".to_string();
        }

        let Some(kind) = self.ctx.get_type(type_id) else {
            return "unknown".to_string();
        };

        match kind {
            TypeKind::Void => "void".to_string(),
            TypeKind::Node => "Node".to_string(),
            TypeKind::String => "string".to_string(),
            TypeKind::Custom(name) => name.clone(),
            TypeKind::Ref(name) => to_pascal_case(name),

            TypeKind::Struct(fields) => {
                if let Some(name) = self.type_names.get(&type_id) {
                    name.clone()
                } else {
                    self.inline_struct(fields)
                }
            }

            TypeKind::Enum(variants) => {
                if let Some(name) = self.type_names.get(&type_id) {
                    name.clone()
                } else {
                    self.inline_enum(variants)
                }
            }

            TypeKind::Array { element, non_empty } => {
                let elem_type = self.type_to_ts(*element);
                if *non_empty {
                    format!("[{}, ...{}[]]", elem_type, elem_type)
                } else {
                    format!("{}[]", elem_type)
                }
            }

            TypeKind::Optional(inner) => {
                let inner_type = self.type_to_ts(*inner);
                format!("{} | null", inner_type)
            }
        }
    }

    fn inline_struct(&self, fields: &BTreeMap<String, FieldInfo>) -> String {
        if fields.is_empty() {
            return "{}".to_string();
        }

        let field_strs: Vec<_> = fields
            .iter()
            .map(|(name, info)| {
                let ts_type = self.type_to_ts(info.type_id);
                let optional = if info.optional { "?" } else { "" };
                format!("{}{}: {}", name, optional, ts_type)
            })
            .collect();

        format!("{{ {} }}", field_strs.join("; "))
    }

    fn inline_enum(&self, variants: &BTreeMap<String, TypeId>) -> String {
        let variant_strs: Vec<_> = variants
            .iter()
            .map(|(name, type_id)| {
                let data_type = self.type_to_ts(*type_id);
                format!("{{ $tag: \"{}\"; $data: {} }}", name, data_type)
            })
            .collect();

        variant_strs.join(" | ")
    }
}

/// Convert a string to PascalCase.
fn to_pascal_case(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = true;

    for c in s.chars() {
        if c == '_' || c == '-' || c == '.' {
            capitalize_next = true;
        } else if capitalize_next {
            result.extend(c.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }

    result
}

/// Convenience function to emit TypeScript from a TypeContext.
pub fn emit_typescript(ctx: &TypeContext) -> String {
    TsEmitter::new(ctx, EmitConfig::default()).emit()
}

/// Emit TypeScript with custom configuration.
pub fn emit_typescript_with_config(ctx: &TypeContext, config: EmitConfig) -> String {
    TsEmitter::new(ctx, config).emit()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_pascal_case_works() {
        assert_eq!(to_pascal_case("foo"), "Foo");
        assert_eq!(to_pascal_case("foo_bar"), "FooBar");
        assert_eq!(to_pascal_case("foo-bar"), "FooBar");
        assert_eq!(to_pascal_case("_"), "");
        assert_eq!(to_pascal_case("FooBar"), "FooBar");
    }

    #[test]
    fn emit_node_type() {
        let ctx = TypeContext::new();
        let output = TsEmitter::new(&ctx, EmitConfig::default()).emit();

        assert!(output.contains("interface Node"));
        assert!(output.contains("kind: string"));
    }
}
