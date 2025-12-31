//! TypeScript type emitter from bytecode Module.
//!
//! Converts compiled bytecode back to TypeScript declarations.
//! Used as a test oracle and for generating types from .ptkq files.

use std::collections::hash_map::Entry;
use std::collections::{BTreeSet, HashMap, HashSet};

use plotnik_core::utils::to_pascal_case;

use crate::bytecode::module::{Module, StringsView, TypesView};
use crate::bytecode::type_meta::{TypeDef, TypeKind};
use crate::bytecode::{EntrypointsView, QTypeId};

/// Configuration for TypeScript emission.
#[derive(Clone, Debug)]
pub struct EmitConfig {
    /// Whether to export types
    pub export: bool,
    /// Whether to emit the Node type definition
    pub emit_node_type: bool,
    /// Use verbose node representation (with kind, text, etc.)
    pub verbose_nodes: bool,
}

impl Default for EmitConfig {
    fn default() -> Self {
        Self {
            export: true,
            emit_node_type: true,
            verbose_nodes: false,
        }
    }
}

/// TypeScript emitter from bytecode module.
pub struct TsEmitter<'a> {
    types: TypesView<'a>,
    strings: StringsView<'a>,
    entrypoints: EntrypointsView<'a>,
    config: EmitConfig,

    /// TypeId -> assigned name mapping
    type_names: HashMap<QTypeId, String>,
    /// Names already used (for collision avoidance)
    used_names: BTreeSet<String>,
    /// Track which builtin types are referenced
    node_referenced: bool,
    /// Track which types have been emitted
    emitted: HashSet<QTypeId>,
    /// Types visited during builtin reference collection (cycle detection)
    refs_visited: HashSet<QTypeId>,
    /// Output buffer
    output: String,
}

impl<'a> TsEmitter<'a> {
    pub fn new(module: &'a Module, config: EmitConfig) -> Self {
        Self {
            types: module.types(),
            strings: module.strings(),
            entrypoints: module.entrypoints(),
            config,
            type_names: HashMap::new(),
            used_names: BTreeSet::new(),
            node_referenced: false,
            emitted: HashSet::new(),
            refs_visited: HashSet::new(),
            output: String::new(),
        }
    }

    /// Emit TypeScript for all entrypoint types.
    pub fn emit(mut self) -> String {
        self.prepare_emission();

        // Collect all entrypoints and their result types
        let mut primary_names: HashMap<QTypeId, String> = HashMap::new();
        let mut aliases: Vec<(String, QTypeId)> = Vec::new();

        for i in 0..self.entrypoints.len() {
            let ep = self.entrypoints.get(i);
            let name = self.strings.get(ep.name).to_string();
            let type_id = ep.result_type;

            match primary_names.entry(type_id) {
                Entry::Vacant(e) => {
                    e.insert(name);
                }
                Entry::Occupied(_) => {
                    aliases.push((name, type_id));
                }
            }
        }

        // Collect all reachable types starting from entrypoints
        let mut to_emit = HashSet::new();
        for i in 0..self.entrypoints.len() {
            let ep = self.entrypoints.get(i);
            self.collect_reachable_types(ep.result_type, &mut to_emit);
        }

        // Emit in topological order
        for type_id in self.sort_topologically(to_emit) {
            if let Some(def_name) = primary_names.get(&type_id) {
                self.emit_type_definition(def_name, type_id);
            } else {
                self.emit_generated_or_custom(type_id);
            }
        }

        // Emit aliases
        for (alias_name, type_id) in aliases {
            if let Some(primary_name) = primary_names.get(&type_id) {
                self.emit_type_alias(&alias_name, primary_name);
            }
        }

        self.output
    }

    fn prepare_emission(&mut self) {
        // Reserve entrypoint names to avoid collisions
        for i in 0..self.entrypoints.len() {
            let ep = self.entrypoints.get(i);
            let name = self.strings.get(ep.name);
            self.used_names.insert(to_pascal_case(name));
        }

        // Assign names to named types from TypeNames section
        for i in 0..self.types.names_count() {
            let type_name = self.types.get_name(i);
            let name = self.strings.get(type_name.name);
            self.type_names
                .insert(type_name.type_id, to_pascal_case(name));
        }

        // Assign names to struct/enum types that need them but don't have names
        self.assign_generated_names();

        // Collect builtin references
        self.collect_builtin_references();

        // Emit Node interface if referenced
        if self.config.emit_node_type && self.node_referenced {
            self.emit_node_interface();
        }
    }

    fn assign_generated_names(&mut self) {
        // Collect naming contexts from entrypoints → fields
        let mut contexts: HashMap<QTypeId, NamingContext> = HashMap::new();

        for i in 0..self.entrypoints.len() {
            let ep = self.entrypoints.get(i);
            let def_name = self.strings.get(ep.name);
            self.collect_naming_contexts(
                ep.result_type,
                &NamingContext {
                    def_name: def_name.to_string(),
                    field_name: None,
                },
                &mut contexts,
            );
        }

        // Assign names to types that need them
        for i in 0..self.types.defs_count() {
            let type_id = QTypeId::from_custom_index(i);
            if self.type_names.contains_key(&type_id) {
                continue;
            }

            let type_def = self.types.get_def(i);
            if !self.needs_generated_name(&type_def) {
                continue;
            }

            let name = if let Some(ctx) = contexts.get(&type_id) {
                self.generate_contextual_name(ctx)
            } else {
                self.generate_fallback_name(&type_def)
            };
            self.type_names.insert(type_id, name);
        }
    }

    fn collect_naming_contexts(
        &self,
        type_id: QTypeId,
        ctx: &NamingContext,
        contexts: &mut HashMap<QTypeId, NamingContext>,
    ) {
        if type_id.is_builtin() || contexts.contains_key(&type_id) {
            return;
        }

        let Some(type_def) = self.types.get(type_id) else {
            return;
        };

        let Some(kind) = type_def.type_kind() else {
            return;
        };

        match kind {
            TypeKind::Struct => {
                contexts.entry(type_id).or_insert_with(|| ctx.clone());
                for member in self.types.members_of(&type_def) {
                    let field_name = self.strings.get(member.name);
                    // Unwrap Optional wrappers to get the actual type
                    let (inner_type, _) = self.unwrap_optional(member.type_id);
                    let field_ctx = NamingContext {
                        def_name: ctx.def_name.clone(),
                        field_name: Some(field_name.to_string()),
                    };
                    self.collect_naming_contexts(inner_type, &field_ctx, contexts);
                }
            }
            TypeKind::Enum => {
                contexts.entry(type_id).or_insert_with(|| ctx.clone());
            }
            TypeKind::ArrayZeroOrMore | TypeKind::ArrayOneOrMore => {
                let inner = QTypeId(type_def.data);
                self.collect_naming_contexts(inner, ctx, contexts);
            }
            TypeKind::Optional => {
                let inner = QTypeId(type_def.data);
                self.collect_naming_contexts(inner, ctx, contexts);
            }
            TypeKind::Alias => {
                // Aliases don't need contexts
            }
        }
    }

    fn collect_builtin_references(&mut self) {
        for i in 0..self.entrypoints.len() {
            let ep = self.entrypoints.get(i);
            self.collect_refs_recursive(ep.result_type);
        }
    }

    fn collect_refs_recursive(&mut self, type_id: QTypeId) {
        if type_id == QTypeId::NODE {
            self.node_referenced = true;
            return;
        }
        if type_id == QTypeId::STRING || type_id == QTypeId::VOID {
            return;
        }

        // Cycle detection
        if !self.refs_visited.insert(type_id) {
            return;
        }

        let Some(type_def) = self.types.get(type_id) else {
            return;
        };

        let Some(kind) = type_def.type_kind() else {
            return;
        };

        match kind {
            TypeKind::Struct | TypeKind::Enum => {
                let member_types: Vec<_> = self
                    .types
                    .members_of(&type_def)
                    .map(|m| m.type_id)
                    .collect();
                for ty in member_types {
                    self.collect_refs_recursive(ty);
                }
            }
            TypeKind::ArrayZeroOrMore | TypeKind::ArrayOneOrMore | TypeKind::Optional => {
                self.collect_refs_recursive(QTypeId(type_def.data));
            }
            TypeKind::Alias => {
                // Alias to Node
                self.node_referenced = true;
            }
        }
    }

    fn sort_topologically(&self, types: HashSet<QTypeId>) -> Vec<QTypeId> {
        let mut deps: HashMap<QTypeId, HashSet<QTypeId>> = HashMap::new();
        let mut rdeps: HashMap<QTypeId, HashSet<QTypeId>> = HashMap::new();

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
        let mut queue: Vec<QTypeId> = deps
            .iter()
            .filter(|(_, d)| d.is_empty())
            .map(|(&tid, _)| tid)
            .collect();

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

    fn collect_reachable_types(&self, type_id: QTypeId, out: &mut HashSet<QTypeId>) {
        if type_id.is_builtin() || out.contains(&type_id) {
            return;
        }

        let Some(type_def) = self.types.get(type_id) else {
            return;
        };

        let Some(kind) = type_def.type_kind() else {
            return;
        };

        match kind {
            TypeKind::Struct => {
                out.insert(type_id);
                for member in self.types.members_of(&type_def) {
                    self.collect_reachable_types(member.type_id, out);
                }
            }
            TypeKind::Enum => {
                out.insert(type_id);
                for member in self.types.members_of(&type_def) {
                    // For enum variants, recurse into payload fields but don't
                    // add the payload struct itself - it will be inlined.
                    self.collect_enum_variant_refs(member.type_id, out);
                }
            }
            TypeKind::Alias => {
                out.insert(type_id);
            }
            TypeKind::ArrayZeroOrMore | TypeKind::ArrayOneOrMore => {
                self.collect_reachable_types(QTypeId(type_def.data), out);
            }
            TypeKind::Optional => {
                self.collect_reachable_types(QTypeId(type_def.data), out);
            }
        }
    }

    /// Collect reachable types from enum variant payloads.
    /// Recurses into struct fields but doesn't add the payload struct itself.
    fn collect_enum_variant_refs(&self, type_id: QTypeId, out: &mut HashSet<QTypeId>) {
        if type_id.is_builtin() {
            return;
        }

        let Some(type_def) = self.types.get(type_id) else {
            return;
        };

        let Some(kind) = type_def.type_kind() else {
            return;
        };

        match kind {
            TypeKind::Struct => {
                // DON'T add the struct - it will be inlined as $data.
                // But DO recurse into its fields to find named types.
                for member in self.types.members_of(&type_def) {
                    self.collect_reachable_types(member.type_id, out);
                }
            }
            _ => {
                // For non-struct payloads (shouldn't happen normally),
                // fall back to regular collection.
                self.collect_reachable_types(type_id, out);
            }
        }
    }

    fn get_direct_deps(&self, type_id: QTypeId) -> Vec<QTypeId> {
        let Some(type_def) = self.types.get(type_id) else {
            return vec![];
        };

        let Some(kind) = type_def.type_kind() else {
            return vec![];
        };

        match kind {
            TypeKind::Struct | TypeKind::Enum => self
                .types
                .members_of(&type_def)
                .flat_map(|member| self.unwrap_for_deps(member.type_id))
                .collect(),
            TypeKind::ArrayZeroOrMore | TypeKind::ArrayOneOrMore => {
                self.unwrap_for_deps(QTypeId(type_def.data))
            }
            TypeKind::Optional => self.unwrap_for_deps(QTypeId(type_def.data)),
            TypeKind::Alias => vec![],
        }
    }

    fn unwrap_for_deps(&self, type_id: QTypeId) -> Vec<QTypeId> {
        if type_id.is_builtin() {
            return vec![];
        }

        let Some(type_def) = self.types.get(type_id) else {
            return vec![];
        };

        let Some(kind) = type_def.type_kind() else {
            return vec![];
        };

        match kind {
            TypeKind::ArrayZeroOrMore | TypeKind::ArrayOneOrMore | TypeKind::Optional => {
                self.unwrap_for_deps(QTypeId(type_def.data))
            }
            TypeKind::Struct | TypeKind::Enum | TypeKind::Alias => vec![type_id],
        }
    }

    fn emit_generated_or_custom(&mut self, type_id: QTypeId) {
        if self.emitted.contains(&type_id) || type_id.is_builtin() {
            return;
        }

        let Some(type_def) = self.types.get(type_id) else {
            return;
        };

        // Check if this is an alias type (custom type annotation)
        if type_def.is_alias() {
            if let Some(name) = self.type_names.get(&type_id).cloned() {
                self.emit_custom_type_alias(&name);
                self.emitted.insert(type_id);
            }
            return;
        }

        // Check if we have a generated name
        if let Some(name) = self.type_names.get(&type_id).cloned() {
            self.emit_generated_type_def(type_id, &name);
        }
    }

    fn emit_generated_type_def(&mut self, type_id: QTypeId, name: &str) {
        self.emitted.insert(type_id);
        let export = if self.config.export { "export " } else { "" };

        let Some(type_def) = self.types.get(type_id) else {
            return;
        };

        let Some(kind) = type_def.type_kind() else {
            return;
        };

        match kind {
            TypeKind::Struct => self.emit_interface(name, &type_def, export),
            TypeKind::Enum => self.emit_tagged_union(name, &type_def, export),
            _ => {}
        }
    }

    fn emit_type_definition(&mut self, name: &str, type_id: QTypeId) {
        self.emitted.insert(type_id);
        let export = if self.config.export { "export " } else { "" };
        let type_name = to_pascal_case(name);

        let Some(type_def) = self.types.get(type_id) else {
            // Builtin type - emit as alias
            let ts_type = self.type_to_ts(type_id);
            self.output
                .push_str(&format!("{}type {} = {};\n\n", export, type_name, ts_type));
            return;
        };

        let Some(kind) = type_def.type_kind() else {
            return;
        };

        match kind {
            TypeKind::Struct => self.emit_interface(&type_name, &type_def, export),
            TypeKind::Enum => self.emit_tagged_union(&type_name, &type_def, export),
            _ => {
                let ts_type = self.type_to_ts(type_id);
                self.output
                    .push_str(&format!("{}type {} = {};\n\n", export, type_name, ts_type));
            }
        }
    }

    fn emit_interface(&mut self, name: &str, type_def: &TypeDef, export: &str) {
        self.output
            .push_str(&format!("{}interface {} {{\n", export, name));

        // Collect fields and sort by name
        let mut fields: Vec<(String, QTypeId, bool)> = self
            .types
            .members_of(type_def)
            .map(|member| {
                let field_name = self.strings.get(member.name).to_string();
                let (inner_type, optional) = self.unwrap_optional(member.type_id);
                (field_name, inner_type, optional)
            })
            .collect();
        fields.sort_by(|a, b| a.0.cmp(&b.0));

        for (field_name, field_type, optional) in fields {
            let ts_type = self.type_to_ts(field_type);
            let opt_marker = if optional { "?" } else { "" };
            self.output
                .push_str(&format!("  {}{}: {};\n", field_name, opt_marker, ts_type));
        }

        self.output.push_str("}\n\n");
    }

    fn emit_tagged_union(&mut self, name: &str, type_def: &TypeDef, export: &str) {
        let mut variant_types = Vec::new();

        for member in self.types.members_of(type_def) {
            let variant_name = self.strings.get(member.name);
            let variant_type_name = format!("{}{}", name, to_pascal_case(variant_name));
            variant_types.push(variant_type_name.clone());

            let data_str = self.inline_data_type(member.type_id);
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

    fn emit_type_alias(&mut self, alias_name: &str, target_name: &str) {
        let export = if self.config.export { "export " } else { "" };
        self.output.push_str(&format!(
            "{}type {} = {};\n\n",
            export, alias_name, target_name
        ));
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

    fn type_to_ts(&self, type_id: QTypeId) -> String {
        match type_id {
            QTypeId::VOID => "void".to_string(),
            QTypeId::NODE => "Node".to_string(),
            QTypeId::STRING => "string".to_string(),
            _ => self.custom_type_to_ts(type_id),
        }
    }

    fn custom_type_to_ts(&self, type_id: QTypeId) -> String {
        let Some(type_def) = self.types.get(type_id) else {
            return "unknown".to_string();
        };

        let Some(kind) = type_def.type_kind() else {
            return "unknown".to_string();
        };

        match kind {
            TypeKind::Struct | TypeKind::Enum => {
                if let Some(name) = self.type_names.get(&type_id) {
                    name.clone()
                } else {
                    self.inline_composite(type_id, &type_def, &kind)
                }
            }
            TypeKind::Alias => {
                if let Some(name) = self.type_names.get(&type_id) {
                    name.clone()
                } else {
                    "Node".to_string()
                }
            }
            TypeKind::ArrayZeroOrMore => {
                let elem_type = self.type_to_ts(QTypeId(type_def.data));
                format!("{}[]", elem_type)
            }
            TypeKind::ArrayOneOrMore => {
                let elem_type = self.type_to_ts(QTypeId(type_def.data));
                format!("[{}, ...{}[]]", elem_type, elem_type)
            }
            TypeKind::Optional => {
                let inner_type = self.type_to_ts(QTypeId(type_def.data));
                format!("{} | null", inner_type)
            }
        }
    }

    fn inline_composite(&self, _type_id: QTypeId, type_def: &TypeDef, kind: &TypeKind) -> String {
        match kind {
            TypeKind::Struct => self.inline_struct(type_def),
            TypeKind::Enum => self.inline_enum(type_def),
            _ => "unknown".to_string(),
        }
    }

    fn inline_struct(&self, type_def: &TypeDef) -> String {
        if type_def.count == 0 {
            return "{}".to_string();
        }

        let mut fields: Vec<(String, QTypeId, bool)> = self
            .types
            .members_of(type_def)
            .map(|member| {
                let field_name = self.strings.get(member.name).to_string();
                let (inner_type, optional) = self.unwrap_optional(member.type_id);
                (field_name, inner_type, optional)
            })
            .collect();
        fields.sort_by(|a, b| a.0.cmp(&b.0));

        let field_strs: Vec<String> = fields
            .iter()
            .map(|(name, ty, opt)| {
                let ts_type = self.type_to_ts(*ty);
                let opt_marker = if *opt { "?" } else { "" };
                format!("{}{}: {}", name, opt_marker, ts_type)
            })
            .collect();

        format!("{{ {} }}", field_strs.join("; "))
    }

    fn inline_enum(&self, type_def: &TypeDef) -> String {
        let variant_strs: Vec<String> = self
            .types
            .members_of(type_def)
            .map(|member| {
                let name = self.strings.get(member.name);
                let data_type = self.type_to_ts(member.type_id);
                format!("{{ $tag: \"{}\"; $data: {} }}", name, data_type)
            })
            .collect();

        variant_strs.join(" | ")
    }

    fn inline_data_type(&self, type_id: QTypeId) -> String {
        if type_id == QTypeId::VOID {
            return "{}".to_string();
        }

        let Some(type_def) = self.types.get(type_id) else {
            return self.type_to_ts(type_id);
        };

        let Some(kind) = type_def.type_kind() else {
            return self.type_to_ts(type_id);
        };

        if kind == TypeKind::Struct {
            self.inline_struct(&type_def)
        } else {
            self.type_to_ts(type_id)
        }
    }

    /// Unwrap Optional wrappers and return (inner_type, is_optional).
    fn unwrap_optional(&self, type_id: QTypeId) -> (QTypeId, bool) {
        if type_id.is_builtin() {
            return (type_id, false);
        }
        let Some(type_def) = self.types.get(type_id) else {
            return (type_id, false);
        };
        if type_def.type_kind() != Some(TypeKind::Optional) {
            return (type_id, false);
        }
        (QTypeId(type_def.data), true)
    }

    fn needs_generated_name(&self, type_def: &TypeDef) -> bool {
        matches!(
            type_def.type_kind(),
            Some(TypeKind::Struct) | Some(TypeKind::Enum)
        )
    }

    fn generate_contextual_name(&mut self, ctx: &NamingContext) -> String {
        let base = if let Some(field) = &ctx.field_name {
            format!("{}{}", to_pascal_case(&ctx.def_name), to_pascal_case(field))
        } else {
            to_pascal_case(&ctx.def_name)
        };
        self.unique_name(&base)
    }

    fn generate_fallback_name(&mut self, type_def: &TypeDef) -> String {
        let base = match type_def.type_kind() {
            Some(TypeKind::Struct) => "Struct",
            Some(TypeKind::Enum) => "Enum",
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
}

#[derive(Clone, Debug)]
struct NamingContext {
    def_name: String,
    field_name: Option<String>,
}

/// Emit TypeScript from a bytecode module.
pub fn emit_typescript(module: &Module) -> String {
    TsEmitter::new(module, EmitConfig::default()).emit()
}

/// Emit TypeScript from a bytecode module with custom config.
pub fn emit_typescript_with_config(module: &Module, config: EmitConfig) -> String {
    TsEmitter::new(module, config).emit()
}
