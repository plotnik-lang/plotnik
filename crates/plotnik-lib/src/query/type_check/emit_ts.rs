//! TypeScript type emitter for testing type inference.
//!
//! Converts inferred types to TypeScript declarations.
//! Used as a test oracle to verify type inference correctness.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use plotnik_core::Interner;

use super::context::TypeContext;
use super::symbol::Symbol;
use super::types::{FieldInfo, TYPE_NODE, TYPE_STRING, TYPE_VOID, TypeId, TypeKind};

/// Naming context for synthetic type names: (DefName, FieldName)
#[derive(Clone, Debug)]
struct NamingContext {
    def_name: String,
    field_name: Option<String>,
}

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
    interner: &'a Interner,
    config: EmitConfig,

    /// Generated type names, to avoid collisions
    used_names: BTreeSet<String>,
    /// TypeId -> generated name mapping
    type_names: HashMap<TypeId, String>,

    /// Track which builtin types are referenced
    referenced_builtins: HashSet<TypeId>,
    /// Track which types have been emitted
    emitted: HashSet<TypeId>,
    /// Output buffer
    output: String,
}

impl<'a> TsEmitter<'a> {
    pub fn new(ctx: &'a TypeContext, interner: &'a Interner, config: EmitConfig) -> Self {
        Self {
            ctx,
            interner,
            config,
            used_names: BTreeSet::new(),
            type_names: HashMap::new(),
            referenced_builtins: HashSet::new(),
            emitted: HashSet::new(),
            output: String::new(),
        }
    }

    /// Emit TypeScript for all definition types.
    pub fn emit(mut self) -> String {
        self.prepare_emission();

        // Collect definition names for lookup
        let def_names: HashMap<TypeId, String> = self
            .ctx
            .iter_def_types()
            .map(|(def_id, type_id)| {
                (
                    type_id,
                    self.ctx.def_name(self.interner, def_id).to_string(),
                )
            })
            .collect();

        // Collect all reachable types starting from definitions
        let mut to_emit = HashSet::new();
        for (_, type_id) in self.ctx.iter_def_types() {
            self.collect_reachable_types(type_id, &mut to_emit);
        }

        // Emit in topological order
        for type_id in self.sort_topologically(to_emit) {
            if let Some(def_name) = def_names.get(&type_id) {
                self.emit_type_definition(def_name, type_id);
            } else {
                self.emit_generated_or_custom(type_id);
            }
        }

        self.output
    }

    /// Emit TypeScript for a single definition.
    pub fn emit_single(mut self, name: &str, type_id: TypeId) -> String {
        self.prepare_emission();

        let mut to_emit = HashSet::new();
        self.collect_reachable_types(type_id, &mut to_emit);

        let sorted = self.sort_topologically(to_emit);

        // Emit dependencies (everything except the root)
        for &dep_id in &sorted {
            if dep_id != type_id {
                self.emit_generated_or_custom(dep_id);
            }
        }

        // Emit the main definition
        self.emit_type_definition(name, type_id);
        self.output
    }

    fn prepare_emission(&mut self) {
        self.assign_generated_names();
        self.collect_builtin_references();

        if self.config.emit_node_type && self.referenced_builtins.contains(&TYPE_NODE) {
            self.emit_node_interface();
        }
    }

    fn assign_generated_names(&mut self) {
        // 1. Reserve definition names to avoid collisions
        for (def_id, _) in self.ctx.iter_def_types() {
            let name = self.ctx.def_name(self.interner, def_id);
            self.used_names.insert(to_pascal_case(name));
        }

        // 2. Collect naming contexts (path from definition to type)
        let mut contexts = HashMap::new();
        for (def_id, type_id) in self.ctx.iter_def_types() {
            let def_name = self.ctx.def_name(self.interner, def_id);
            self.collect_naming_contexts(
                type_id,
                &NamingContext {
                    def_name: def_name.to_string(),
                    field_name: None,
                },
                &mut contexts,
            );
        }

        // 3. Assign names to types that need them
        for (id, kind) in self.ctx.iter_types() {
            if !self.needs_generated_name(kind) || self.type_names.contains_key(&id) {
                continue;
            }

            let name = if let Some(ctx) = contexts.get(&id) {
                self.generate_contextual_name(ctx)
            } else {
                self.generate_fallback_name(kind)
            };
            self.type_names.insert(id, name);
        }
    }

    fn collect_naming_contexts(
        &self,
        type_id: TypeId,
        ctx: &NamingContext,
        contexts: &mut HashMap<TypeId, NamingContext>,
    ) {
        if type_id.is_builtin() || contexts.contains_key(&type_id) {
            return;
        }

        let Some(kind) = self.ctx.get_type(type_id) else {
            return;
        };

        match kind {
            TypeKind::Struct(fields) => {
                contexts.entry(type_id).or_insert_with(|| ctx.clone());
                for (&field_sym, info) in fields {
                    let field_name = self.interner.resolve(field_sym);
                    let field_ctx = NamingContext {
                        def_name: ctx.def_name.clone(),
                        field_name: Some(field_name.to_string()),
                    };
                    self.collect_naming_contexts(info.type_id, &field_ctx, contexts);
                }
            }
            TypeKind::Enum(_) => {
                contexts.entry(type_id).or_insert_with(|| ctx.clone());
            }
            TypeKind::Array { element, .. } => {
                self.collect_naming_contexts(*element, ctx, contexts);
            }
            TypeKind::Optional(inner) => {
                self.collect_naming_contexts(*inner, ctx, contexts);
            }
            _ => {}
        }
    }

    fn collect_builtin_references(&mut self) {
        for (_, type_id) in self.ctx.iter_def_types() {
            self.collect_refs_recursive(type_id);
        }
    }

    fn collect_refs_recursive(&mut self, type_id: TypeId) {
        if type_id == TYPE_NODE || type_id == TYPE_STRING {
            self.referenced_builtins.insert(type_id);
            return;
        }
        if type_id == TYPE_VOID {
            return;
        }

        let Some(kind) = self.ctx.get_type(type_id) else {
            return;
        };

        match kind {
            TypeKind::Node | TypeKind::Custom(_) => {
                self.referenced_builtins.insert(TYPE_NODE);
            }
            TypeKind::String => {
                self.referenced_builtins.insert(TYPE_STRING);
            }
            TypeKind::Struct(fields) => {
                fields
                    .values()
                    .for_each(|info| self.collect_refs_recursive(info.type_id));
            }
            TypeKind::Enum(variants) => {
                variants
                    .values()
                    .for_each(|&tid| self.collect_refs_recursive(tid));
            }
            TypeKind::Array { element, .. } => self.collect_refs_recursive(*element),
            TypeKind::Optional(inner) => self.collect_refs_recursive(*inner),
            _ => {}
        }
    }

    fn sort_topologically(&self, types: HashSet<TypeId>) -> Vec<TypeId> {
        let mut deps: HashMap<TypeId, HashSet<TypeId>> = HashMap::new();
        let mut rdeps: HashMap<TypeId, HashSet<TypeId>> = HashMap::new();

        for &tid in &types {
            deps.entry(tid).or_default();
            rdeps.entry(tid).or_default();
        }

        // Build dependency graph
        for &tid in &types {
            for dep in self.get_direct_deps(tid) {
                if types.contains(&dep) && dep != tid {
                    deps.entry(tid).or_default().insert(dep);
                    rdeps.entry(dep).or_default().insert(tid);
                }
            }
        }

        // Kahn's algorithm
        let mut result = Vec::with_capacity(types.len());
        let mut queue: Vec<TypeId> = deps
            .iter()
            .filter(|(_, d)| d.is_empty())
            .map(|(&tid, _)| tid)
            .collect();

        // Sort for deterministic output
        queue.sort_by_key(|tid| tid.0);

        while let Some(tid) = queue.pop() {
            result.push(tid);
            if let Some(dependents) = rdeps.get(&tid) {
                for &dependent in dependents {
                    if let Some(dep_set) = deps.get_mut(&dependent) {
                        dep_set.remove(&tid);
                        if dep_set.is_empty() {
                            queue.push(dependent);
                            queue.sort_by_key(|t| t.0);
                        }
                    }
                }
            }
        }

        result
    }

    fn collect_reachable_types(&self, type_id: TypeId, out: &mut HashSet<TypeId>) {
        if type_id.is_builtin() || out.contains(&type_id) {
            return;
        }

        let Some(kind) = self.ctx.get_type(type_id) else {
            return;
        };

        match kind {
            TypeKind::Struct(fields) => {
                out.insert(type_id);
                for info in fields.values() {
                    self.collect_reachable_types(info.type_id, out);
                }
            }
            TypeKind::Enum(_) | TypeKind::Custom(_) => {
                out.insert(type_id);
            }
            TypeKind::Array { element, .. } => self.collect_reachable_types(*element, out),
            TypeKind::Optional(inner) => self.collect_reachable_types(*inner, out),
            _ => {}
        }
    }

    fn get_direct_deps(&self, type_id: TypeId) -> Vec<TypeId> {
        let Some(kind) = self.ctx.get_type(type_id) else {
            return vec![];
        };
        match kind {
            TypeKind::Struct(fields) => fields
                .values()
                .flat_map(|info| self.unwrap_for_deps(info.type_id))
                .collect(),
            TypeKind::Enum(variants) => variants
                .values()
                .flat_map(|&tid| self.unwrap_for_deps(tid))
                .collect(),
            TypeKind::Array { element, .. } => self.unwrap_for_deps(*element),
            TypeKind::Optional(inner) => self.unwrap_for_deps(*inner),
            _ => vec![],
        }
    }

    fn unwrap_for_deps(&self, type_id: TypeId) -> Vec<TypeId> {
        if type_id.is_builtin() {
            return vec![];
        }
        let Some(kind) = self.ctx.get_type(type_id) else {
            return vec![];
        };
        match kind {
            TypeKind::Array { element, .. } => self.unwrap_for_deps(*element),
            TypeKind::Optional(inner) => self.unwrap_for_deps(*inner),
            TypeKind::Struct(_) | TypeKind::Enum(_) | TypeKind::Custom(_) => vec![type_id],
            _ => vec![],
        }
    }

    fn emit_generated_or_custom(&mut self, type_id: TypeId) {
        if self.emitted.contains(&type_id) || type_id.is_builtin() {
            return;
        }

        if let Some(name) = self.type_names.get(&type_id).cloned() {
            self.emit_generated_type_def(type_id, &name);
        } else if let Some(TypeKind::Custom(sym)) = self.ctx.get_type(type_id) {
            self.emit_custom_type_alias(self.interner.resolve(*sym));
            self.emitted.insert(type_id);
        }
    }

    fn emit_generated_type_def(&mut self, type_id: TypeId, name: &str) {
        self.emitted.insert(type_id);
        let export = if self.config.export { "export " } else { "" };
        let Some(kind) = self.ctx.get_type(type_id) else {
            return;
        };

        match kind {
            TypeKind::Struct(fields) => self.emit_interface(name, fields, export),
            TypeKind::Enum(variants) => self.emit_tagged_union(name, variants, export),
            _ => {}
        }
    }

    fn emit_type_definition(&mut self, name: &str, type_id: TypeId) {
        self.emitted.insert(type_id);
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
                let ts_type = self.type_to_ts(type_id);
                self.output
                    .push_str(&format!("{}type {} = {};\n\n", export, type_name, ts_type));
            }
        }
    }

    fn emit_interface(&mut self, name: &str, fields: &BTreeMap<Symbol, FieldInfo>, export: &str) {
        self.output
            .push_str(&format!("{}interface {} {{\n", export, name));

        for (&sym, info) in self.sort_map_by_name(fields) {
            let field_name = self.interner.resolve(sym);
            let ts_type = self.type_to_ts(info.type_id);
            let optional = if info.optional { "?" } else { "" };
            self.output
                .push_str(&format!("  {}{}: {};\n", field_name, optional, ts_type));
        }

        self.output.push_str("}\n\n");
    }

    fn emit_tagged_union(&mut self, name: &str, variants: &BTreeMap<Symbol, TypeId>, export: &str) {
        let mut variant_types = Vec::new();

        for (&sym, &type_id) in variants {
            let variant_name = self.interner.resolve(sym);
            let variant_type_name = format!("{}{}", name, to_pascal_case(variant_name));
            variant_types.push(variant_type_name.clone());

            let data_str = self.inline_data_type(type_id);
            self.output.push_str(&format!(
                "{}interface {} {{\n  $tag: \"{}\";\n  $data: {};\n}}\n\n",
                export, variant_type_name, variant_name, data_str
            ));
        }

        let union = variant_types.join(" | ");
        self.output
            .push_str(&format!("{}type {} = {};\n\n", export, name, union));
    }

    fn emit_custom_type_alias(&mut self, name: &str) {
        let export = if self.config.export { "export " } else { "" };
        self.output
            .push_str(&format!("{}type {} = Node;\n\n", export, name));
    }

    fn emit_node_interface(&mut self) {
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

    fn type_to_ts(&self, type_id: TypeId) -> String {
        match type_id {
            TYPE_VOID => return "void".to_string(),
            TYPE_NODE => return "Node".to_string(),
            TYPE_STRING => return "string".to_string(),
            _ => {}
        }

        let Some(kind) = self.ctx.get_type(type_id) else {
            return "unknown".to_string();
        };

        match kind {
            TypeKind::Void => "void".to_string(),
            TypeKind::Node => "Node".to_string(),
            TypeKind::String => "string".to_string(),
            TypeKind::Custom(sym) => self.interner.resolve(*sym).to_string(),
            TypeKind::Ref(def_id) => to_pascal_case(self.ctx.def_name(self.interner, *def_id)),

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
            TypeKind::Optional(inner) => format!("{} | null", self.type_to_ts(*inner)),
        }
    }

    fn inline_struct(&self, fields: &BTreeMap<Symbol, FieldInfo>) -> String {
        if fields.is_empty() {
            return "{}".to_string();
        }

        let field_strs: Vec<_> = self
            .sort_map_by_name(fields)
            .into_iter()
            .map(|(&sym, info)| {
                let name = self.interner.resolve(sym);
                let ts_type = self.type_to_ts(info.type_id);
                let optional = if info.optional { "?" } else { "" };
                format!("{}{}: {}", name, optional, ts_type)
            })
            .collect();

        format!("{{ {} }}", field_strs.join("; "))
    }

    fn inline_enum(&self, variants: &BTreeMap<Symbol, TypeId>) -> String {
        let variant_strs: Vec<_> = self
            .sort_map_by_name(variants)
            .into_iter()
            .map(|(&sym, &type_id)| {
                let name = self.interner.resolve(sym);
                let data_type = self.type_to_ts(type_id);
                format!("{{ $tag: \"{}\"; $data: {} }}", name, data_type)
            })
            .collect();

        variant_strs.join(" | ")
    }

    fn inline_data_type(&self, type_id: TypeId) -> String {
        let Some(kind) = self.ctx.get_type(type_id) else {
            return "unknown".to_string();
        };

        match kind {
            TypeKind::Struct(fields) => self.inline_struct(fields),
            TypeKind::Void => "{}".to_string(),
            _ => self.type_to_ts(type_id),
        }
    }

    fn needs_generated_name(&self, kind: &TypeKind) -> bool {
        matches!(kind, TypeKind::Struct(_) | TypeKind::Enum(_))
    }

    fn generate_contextual_name(&mut self, ctx: &NamingContext) -> String {
        let base = if let Some(field) = &ctx.field_name {
            format!("{}{}", to_pascal_case(&ctx.def_name), to_pascal_case(field))
        } else {
            to_pascal_case(&ctx.def_name)
        };
        self.unique_name(&base)
    }

    fn generate_fallback_name(&mut self, kind: &TypeKind) -> String {
        let base = match kind {
            TypeKind::Struct(_) => "Struct",
            TypeKind::Enum(_) => "Enum",
            _ => "Type",
        };
        self.unique_name(base)
    }

    fn unique_name(&mut self, base: &str) -> String {
        let base = to_pascal_case(base);
        if self.used_names.insert(base.clone()) {
            return base;
        }

        let mut counter = 2;
        loop {
            let name = format!("{}{}", base, counter);
            if self.used_names.insert(name.clone()) {
                return name;
            }
            counter += 1;
        }
    }

    /// Helper to iterate map sorted by resolved symbol name.
    fn sort_map_by_name<'b, T>(&self, map: &'b BTreeMap<Symbol, T>) -> Vec<(&'b Symbol, &'b T)> {
        let mut items: Vec<_> = map.iter().collect();
        items.sort_by_key(|&(&sym, _)| self.interner.resolve(sym));
        items
    }
}

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

pub fn emit_typescript(ctx: &TypeContext, interner: &Interner) -> String {
    TsEmitter::new(ctx, interner, EmitConfig::default()).emit()
}

pub fn emit_typescript_with_config(
    ctx: &TypeContext,
    interner: &Interner,
    config: EmitConfig,
) -> String {
    TsEmitter::new(ctx, interner, config).emit()
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
    fn emit_node_type_only_when_referenced() {
        // Empty context - Node should not be emitted
        let ctx = TypeContext::new();
        let interner = Interner::new();
        let output = TsEmitter::new(&ctx, &interner, EmitConfig::default()).emit();
        assert!(!output.contains("interface Node"));

        // Context with a definition using Node - should emit Node
        let mut ctx = TypeContext::new();
        let mut interner = Interner::new();
        let x_sym = interner.intern("x");
        let mut fields = BTreeMap::new();
        fields.insert(x_sym, FieldInfo::required(TYPE_NODE));
        let struct_id = ctx.intern_type(TypeKind::Struct(fields));
        ctx.set_def_type_by_name(&mut interner, "Q", struct_id);

        let output = TsEmitter::new(&ctx, &interner, EmitConfig::default()).emit();
        assert!(output.contains("interface Node"));
        assert!(output.contains("kind: string"));
    }
}
