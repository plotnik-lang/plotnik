//! Output rendering methods.

use plotnik_core::utils::to_pascal_case;

use plotnik_bytecode::{TypeData, TypeDef, TypeId, TypeKind};

use super::Emitter;

impl Emitter<'_> {
    pub(super) fn prepare_emission(&mut self) {
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

    pub(super) fn emit_generated_or_custom(&mut self, type_id: TypeId) {
        if self.emitted.contains(&type_id) {
            return;
        }

        let Some(type_def) = self.types.get(type_id) else {
            return;
        };

        match type_def.classify() {
            TypeData::Primitive(_) => (),
            TypeData::Wrapper {
                kind: TypeKind::Alias,
                ..
            } => {
                if let Some(name) = self.type_names.get(&type_id).cloned() {
                    self.emit_custom_type_alias(&name);
                    self.emitted.insert(type_id);
                }
            }
            TypeData::Wrapper { .. } => (),
            TypeData::Composite { .. } => {
                if let Some(name) = self.type_names.get(&type_id).cloned() {
                    self.emit_generated_type_def(type_id, &name);
                }
            }
        }
    }

    fn emit_generated_type_def(&mut self, type_id: TypeId, name: &str) {
        self.emitted.insert(type_id);

        let Some(type_def) = self.types.get(type_id) else {
            return;
        };

        match type_def.classify() {
            TypeData::Composite {
                kind: TypeKind::Struct,
                ..
            } => self.emit_interface(name, &type_def),
            TypeData::Composite {
                kind: TypeKind::Enum,
                ..
            } => self.emit_tagged_union(name, &type_def),
            _ => {}
        }
    }

    pub(super) fn emit_type_definition(&mut self, name: &str, type_id: TypeId) {
        self.emitted.insert(type_id);
        let type_name = to_pascal_case(name);

        let Some(type_def) = self.types.get(type_id) else {
            // Builtin type - emit as alias
            let ts_type = self.type_to_ts(type_id);
            self.emit_type_decl(&type_name, &ts_type);
            return;
        };

        match type_def.classify() {
            TypeData::Composite {
                kind: TypeKind::Struct,
                ..
            } => self.emit_interface(&type_name, &type_def),
            TypeData::Composite {
                kind: TypeKind::Enum,
                ..
            } => self.emit_tagged_union(&type_name, &type_def),
            _ => {
                let ts_type = self.type_to_ts(type_id);
                self.emit_type_decl(&type_name, &ts_type);
            }
        }
    }

    /// Emit `export type Name = Body;` with proper coloring.
    pub(super) fn emit_type_decl(&mut self, name: &str, body: &str) {
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
        let mut fields: Vec<(String, TypeId, bool)> = self
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
                "{}  {}{}{}{}: {}{}{};\n",
                c.reset, field_name, c.dim, opt_marker, c.dim, c.reset, ts_type, c.dim
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

            let is_void = self.is_void_type(member.type_id);

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
            // $data field (omit for Void payloads)
            if !is_void {
                let data_str = self.inline_data_type(member.type_id);
                self.output.push_str(&format!(
                    "  $data{}:{} {}{};\n",
                    c.dim, c.reset, data_str, c.dim
                ));
            }
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

    pub(super) fn emit_type_alias(&mut self, alias_name: &str, target_name: &str) {
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
        self.output.push_str(&format!(
            "{}  kind{}:{} string{};\n",
            c.reset, c.dim, c.reset, c.dim
        ));
        self.output.push_str(&format!(
            "{}  text{}:{} string{};\n",
            c.reset, c.dim, c.reset, c.dim
        ));
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
}
