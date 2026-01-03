//! TypeScript type emitter from bytecode Module.
//!
//! Converts compiled bytecode back to TypeScript declarations.
//! Used as a test oracle and for generating types from .ptkq files.

use std::collections::hash_map::Entry;
use std::collections::{BTreeSet, HashMap, HashSet};

use plotnik_core::utils::to_pascal_case;

use crate::bytecode::{
    EntrypointsView, Module, QTypeId, StringsView, TypeDef, TypeKind, TypesView,
};
use crate::Colors;

/// How to represent the void type in TypeScript.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VoidType {
    /// `undefined` - the absence of a value
    #[default]
    Undefined,
    /// `null` - explicit null value
    Null,
}

/// Configuration for TypeScript emission.
#[derive(Clone, Debug)]
pub struct Config {
    /// Whether to export types
    pub export: bool,
    /// Whether to emit the Node type definition
    pub emit_node_type: bool,
    /// Use verbose node representation (with kind, text, etc.)
    pub verbose_nodes: bool,
    /// How to represent the void type
    pub void_type: VoidType,
    /// Color configuration for output
    pub colors: Colors,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            export: true,
            emit_node_type: true,
            verbose_nodes: false,
            void_type: VoidType::default(),
            colors: Colors::OFF,
        }
    }
}

/// TypeScript emitter from bytecode module.
pub struct Emitter<'a> {
    types: TypesView<'a>,
    strings: StringsView<'a>,
    entrypoints: EntrypointsView<'a>,
    config: Config,

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

impl<'a> Emitter<'a> {
    pub fn new(module: &'a Module, config: Config) -> Self {
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

    fn c(&self) -> Colors {
        self.config.colors
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

        // Emit entrypoints with primitive result types (like VOID)
        // These are not in to_emit because collect_reachable_types skips primitives
        for (&type_id, name) in &primary_names {
            if self.emitted.contains(&type_id) {
                continue;
            }
            let Some(type_def) = self.types.get(type_id) else {
                continue;
            };
            if let Some(kind) = type_def.type_kind() && kind.is_primitive() {
                self.emit_type_definition(name, type_id);
            }
        }

        // Emit aliases
        for (alias_name, type_id) in aliases {
            if let Some(primary_name) = primary_names.get(&type_id) {
                self.emit_type_alias(&alias_name, primary_name);
            }
        }

        // Ensure exactly one trailing newline
        self.output.truncate(self.output.trim_end().len());
        self.output.push('\n');
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
            let type_id = QTypeId(i as u16);
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
        if contexts.contains_key(&type_id) {
            return;
        }

        let Some(type_def) = self.types.get(type_id) else {
            return;
        };

        let Some(kind) = type_def.type_kind() else {
            return;
        };

        match kind {
            TypeKind::Void | TypeKind::Node | TypeKind::String | TypeKind::Alias => {}
            TypeKind::Struct => {
                contexts.entry(type_id).or_insert_with(|| ctx.clone());
                for member in self.types.members_of(&type_def) {
                    let field_name = self.strings.get(member.name);
                    let (inner_type, _) = self.types.unwrap_optional(member.type_id);
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
            TypeKind::ArrayZeroOrMore | TypeKind::ArrayOneOrMore | TypeKind::Optional => {
                self.collect_naming_contexts(QTypeId(type_def.data), ctx, contexts);
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
            TypeKind::Node => {
                self.node_referenced = true;
            }
            TypeKind::String | TypeKind::Void => {
                // No action needed for primitives
            }
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
        if out.contains(&type_id) {
            return;
        }

        let Some(type_def) = self.types.get(type_id) else {
            return;
        };

        let Some(kind) = type_def.type_kind() else {
            return;
        };

        match kind {
            TypeKind::Void | TypeKind::Node | TypeKind::String => {}
            TypeKind::Struct => {
                out.insert(type_id);
                for member in self.types.members_of(&type_def) {
                    self.collect_reachable_types(member.type_id, out);
                }
            }
            TypeKind::Enum => {
                out.insert(type_id);
                for member in self.types.members_of(&type_def) {
                    self.collect_enum_variant_refs(member.type_id, out);
                }
            }
            TypeKind::Alias => {
                out.insert(type_id);
            }
            TypeKind::ArrayZeroOrMore | TypeKind::ArrayOneOrMore | TypeKind::Optional => {
                self.collect_reachable_types(QTypeId(type_def.data), out);
            }
        }
    }

    /// Collect reachable types from enum variant payloads.
    /// Recurses into struct fields but doesn't add the payload struct itself.
    fn collect_enum_variant_refs(&self, type_id: QTypeId, out: &mut HashSet<QTypeId>) {
        let Some(type_def) = self.types.get(type_id) else {
            return;
        };

        // For struct payloads, don't add the struct itself (it will be inlined),
        // but recurse into its fields to find named types.
        if type_def.type_kind() == Some(TypeKind::Struct) {
            for member in self.types.members_of(&type_def) {
                self.collect_reachable_types(member.type_id, out);
            }
        } else {
            // For non-struct payloads, fall back to regular collection.
            self.collect_reachable_types(type_id, out);
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
            TypeKind::Void | TypeKind::Node | TypeKind::String | TypeKind::Alias => vec![],
            TypeKind::Struct | TypeKind::Enum => self
                .types
                .members_of(&type_def)
                .flat_map(|member| self.unwrap_for_deps(member.type_id))
                .collect(),
            TypeKind::ArrayZeroOrMore | TypeKind::ArrayOneOrMore | TypeKind::Optional => {
                self.unwrap_for_deps(QTypeId(type_def.data))
            }
        }
    }

    fn unwrap_for_deps(&self, type_id: QTypeId) -> Vec<QTypeId> {
        let Some(type_def) = self.types.get(type_id) else {
            return vec![];
        };

        let Some(kind) = type_def.type_kind() else {
            return vec![];
        };

        match kind {
            TypeKind::Void | TypeKind::Node | TypeKind::String => vec![],
            TypeKind::ArrayZeroOrMore | TypeKind::ArrayOneOrMore | TypeKind::Optional => {
                self.unwrap_for_deps(QTypeId(type_def.data))
            }
            TypeKind::Struct | TypeKind::Enum | TypeKind::Alias => vec![type_id],
        }
    }

    fn emit_generated_or_custom(&mut self, type_id: QTypeId) {
        if self.emitted.contains(&type_id) {
            return;
        }

        let Some(type_def) = self.types.get(type_id) else {
            return;
        };

        let Some(kind) = type_def.type_kind() else {
            return;
        };

        if kind.is_primitive() {
            return;
        }

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

        let Some(type_def) = self.types.get(type_id) else {
            return;
        };

        let Some(kind) = type_def.type_kind() else {
            return;
        };

        match kind {
            TypeKind::Struct => self.emit_interface(name, &type_def),
            TypeKind::Enum => self.emit_tagged_union(name, &type_def),
            _ => {}
        }
    }

    fn emit_type_definition(&mut self, name: &str, type_id: QTypeId) {
        self.emitted.insert(type_id);
        let type_name = to_pascal_case(name);

        let Some(type_def) = self.types.get(type_id) else {
            // Builtin type - emit as alias
            let ts_type = self.type_to_ts(type_id);
            self.emit_type_decl(&type_name, &ts_type);
            return;
        };

        let Some(kind) = type_def.type_kind() else {
            return;
        };

        match kind {
            TypeKind::Struct => self.emit_interface(&type_name, &type_def),
            TypeKind::Enum => self.emit_tagged_union(&type_name, &type_def),
            _ => {
                let ts_type = self.type_to_ts(type_id);
                self.emit_type_decl(&type_name, &ts_type);
            }
        }
    }

    /// Emit `export type Name = Body;` with proper coloring.
    fn emit_type_decl(&mut self, name: &str, body: &str) {
        let c = self.c();
        if self.config.export {
            self.output
                .push_str(&format!("{}export{} ", c.dim, c.reset));
        }
        self.output.push_str(&format!(
            "{}type{} {}{}{} {}={} {}{};\n\n",
            c.dim, c.reset, c.blue, name, c.reset, c.dim, c.reset, body, c.dim
        ));
        self.output.push_str(c.reset);
    }

    fn emit_interface(&mut self, name: &str, type_def: &TypeDef) {
        let c = self.c();

        // Header: export interface Name {
        if self.config.export {
            self.output
                .push_str(&format!("{}export{} ", c.dim, c.reset));
        }
        self.output.push_str(&format!(
            "{}interface{} {}{}{} {}{{\n",
            c.dim, c.reset, c.blue, name, c.reset, c.dim
        ));

        // Collect fields and sort by name
        let mut fields: Vec<(String, QTypeId, bool)> = self
            .types
            .members_of(type_def)
            .map(|member| {
                let field_name = self.strings.get(member.name).to_string();
                let (inner_type, optional) = self.types.unwrap_optional(member.type_id);
                (field_name, inner_type, optional)
            })
            .collect();
        fields.sort_by(|a, b| a.0.cmp(&b.0));

        for (field_name, field_type, optional) in fields {
            let ts_type = self.type_to_ts(field_type);
            let opt_marker = if optional { "?" } else { "" };
            self.output.push_str(&format!(
                "{}  {}{}{}{}: {};\n",
                c.reset, field_name, c.dim, opt_marker, c.dim, ts_type
            ));
        }

        self.output.push_str(&format!("{}}}{}\n\n", c.dim, c.reset));
    }

    fn emit_tagged_union(&mut self, name: &str, type_def: &TypeDef) {
        let c = self.c();
        let mut variant_types = Vec::new();

        for member in self.types.members_of(type_def) {
            let variant_name = self.strings.get(member.name);
            let variant_type_name = format!("{}{}", name, to_pascal_case(variant_name));
            variant_types.push(variant_type_name.clone());

            let data_str = self.inline_data_type(member.type_id);

            // Header: export interface NameVariant {
            if self.config.export {
                self.output
                    .push_str(&format!("{}export{} ", c.dim, c.reset));
            }
            self.output.push_str(&format!(
                "{}interface{} {}{}{} {}{{\n",
                c.dim, c.reset, c.blue, variant_type_name, c.reset, c.dim
            ));
            // $tag field with green string
            self.output.push_str(&format!(
                "{}  $tag{}:{} {}\"{}\"{}{};{}\n",
                c.reset, c.dim, c.reset, c.green, variant_name, c.reset, c.dim, c.reset
            ));
            // $data field
            self.output.push_str(&format!(
                "  $data{}:{} {}{};\n",
                c.dim, c.reset, data_str, c.dim
            ));
            self.output.push_str(&format!("{}}}{}\n\n", c.dim, c.reset));
        }

        // Union type declaration
        let union = variant_types
            .iter()
            .map(|v| format!("{}{}{}", c.blue, v, c.reset))
            .collect::<Vec<_>>()
            .join(&format!(" {}|{} ", c.dim, c.reset));
        self.emit_type_decl(name, &union);
    }

    fn emit_custom_type_alias(&mut self, name: &str) {
        self.emit_type_decl(name, "Node");
    }

    fn emit_type_alias(&mut self, alias_name: &str, target_name: &str) {
        let c = self.c();
        self.emit_type_decl(alias_name, &format!("{}{}{}", c.blue, target_name, c.reset));
    }

    fn emit_node_interface(&mut self) {
        let c = self.c();

        // Header: export interface Node {
        if self.config.export {
            self.output
                .push_str(&format!("{}export{} ", c.dim, c.reset));
        }
        self.output.push_str(&format!(
            "{}interface{} {}Node{} {}{{\n",
            c.dim, c.reset, c.blue, c.reset, c.dim
        ));

        // kind, text, span fields
        self.output
            .push_str(&format!("{}  kind{}:{} string{};\n", c.reset, c.dim, c.reset, c.dim));
        self.output
            .push_str(&format!("{}  text{}:{} string{};\n", c.reset, c.dim, c.reset, c.dim));
        self.output.push_str(&format!(
            "{}  span{}:{} {}[{}number{}, {}number{}]{};\n",
            c.reset, c.dim, c.reset, c.dim, c.reset, c.dim, c.reset, c.dim, c.dim
        ));

        if self.config.verbose_nodes {
            // startPosition and endPosition share same inline type
            let pos_type = format!(
                "{}{{{} row{}:{} number{}; column{}:{} number{}; {}}}",
                c.dim, c.reset, c.dim, c.reset, c.dim, c.dim, c.reset, c.dim, c.dim
            );
            self.output.push_str(&format!(
                "{}  startPosition{}:{} {}{};\n",
                c.reset, c.dim, c.reset, pos_type, c.dim
            ));
            self.output.push_str(&format!(
                "{}  endPosition{}:{} {}{};\n",
                c.reset, c.dim, c.reset, pos_type, c.dim
            ));
        }

        self.output.push_str(&format!("{}}}{}\n\n", c.dim, c.reset));
    }

    fn type_to_ts(&self, type_id: QTypeId) -> String {
        let c = self.c();
        let Some(type_def) = self.types.get(type_id) else {
            return "unknown".to_string();
        };

        let Some(kind) = type_def.type_kind() else {
            return "unknown".to_string();
        };

        match kind {
            TypeKind::Void => match self.config.void_type {
                VoidType::Undefined => "undefined".to_string(),
                VoidType::Null => "null".to_string(),
            },
            TypeKind::Node => "Node".to_string(),
            TypeKind::String => "string".to_string(),
            TypeKind::Struct | TypeKind::Enum => {
                if let Some(name) = self.type_names.get(&type_id) {
                    format!("{}{}{}", c.blue, name, c.reset)
                } else {
                    self.inline_composite(type_id, &type_def, &kind)
                }
            }
            TypeKind::Alias => {
                if let Some(name) = self.type_names.get(&type_id) {
                    format!("{}{}{}", c.blue, name, c.reset)
                } else {
                    "Node".to_string()
                }
            }
            TypeKind::ArrayZeroOrMore => {
                let elem_type = self.type_to_ts(QTypeId(type_def.data));
                format!("{}{}[]{}", elem_type, c.dim, c.reset)
            }
            TypeKind::ArrayOneOrMore => {
                let elem_type = self.type_to_ts(QTypeId(type_def.data));
                format!(
                    "{}[{}{}{}, ...{}{}{}[]]{}",
                    c.dim, c.reset, elem_type, c.dim, c.reset, elem_type, c.dim, c.reset
                )
            }
            TypeKind::Optional => {
                let inner_type = self.type_to_ts(QTypeId(type_def.data));
                format!("{} {}|{} null", inner_type, c.dim, c.reset)
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
        let c = self.c();
        if type_def.count == 0 {
            return format!("{}{{}}{}", c.dim, c.reset);
        }

        let mut fields: Vec<(String, QTypeId, bool)> = self
            .types
            .members_of(type_def)
            .map(|member| {
                let field_name = self.strings.get(member.name).to_string();
                let (inner_type, optional) = self.types.unwrap_optional(member.type_id);
                (field_name, inner_type, optional)
            })
            .collect();
        fields.sort_by(|a, b| a.0.cmp(&b.0));

        let field_strs: Vec<String> = fields
            .iter()
            .map(|(name, ty, opt)| {
                let ts_type = self.type_to_ts(*ty);
                let opt_marker = if *opt {
                    format!("{}?{}", c.dim, c.reset)
                } else {
                    String::new()
                };
                format!("{}{}{}:{} {}", name, opt_marker, c.dim, c.reset, ts_type)
            })
            .collect();

        format!(
            "{}{{{} {} {}}}{}", c.dim, c.reset, field_strs.join(&format!("{}; ", c.dim)), c.dim, c.reset
        )
    }

    fn inline_enum(&self, type_def: &TypeDef) -> String {
        let c = self.c();
        let variant_strs: Vec<String> = self
            .types
            .members_of(type_def)
            .map(|member| {
                let name = self.strings.get(member.name);
                let data_type = self.type_to_ts(member.type_id);
                format!(
                    "{}{{{} $tag{}:{} {}\"{}\"{}{}; $data{}:{} {} {}}}{}",
                    c.dim, c.reset, c.dim, c.reset, c.green, name, c.reset, c.dim, c.dim, c.reset, data_type, c.dim, c.reset
                )
            })
            .collect();

        variant_strs.join(&format!(" {}|{} ", c.dim, c.reset))
    }

    fn inline_data_type(&self, type_id: QTypeId) -> String {
        let c = self.c();
        let Some(type_def) = self.types.get(type_id) else {
            return self.type_to_ts(type_id);
        };

        let Some(kind) = type_def.type_kind() else {
            return self.type_to_ts(type_id);
        };

        if kind == TypeKind::Void {
            return format!("{}{{}}{}", c.dim, c.reset);
        }

        if kind == TypeKind::Struct {
            self.inline_struct(&type_def)
        } else {
            self.type_to_ts(type_id)
        }
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
pub fn emit(module: &Module) -> String {
    Emitter::new(module, Config::default()).emit()
}

/// Emit TypeScript from a bytecode module with custom config.
pub fn emit_with_config(module: &Module, config: Config) -> String {
    Emitter::new(module, config).emit()
}
