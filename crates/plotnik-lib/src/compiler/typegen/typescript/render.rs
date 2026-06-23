//! Output rendering methods.

use crate::core::utils::to_pascal_case;

use crate::bytecode::{TypeDef, TypeDefKind, TypeId, TypeKind};

use super::Emitter;

impl Emitter<'_> {
    pub(super) fn assign_names(&mut self) {
        // Entrypoint names are reserved first so generated names don't collide with them.
        for i in 0..self.entrypoints.len() {
            let ep = self.entrypoints.get(i);
            let name = self.strings.get(ep.name());
            self.used_names.insert(to_pascal_case(name));
        }

        for i in 0..self.types.names_count() {
            let type_name = self.types.get_name(i);
            let name = self.strings.get(type_name.name_id);
            self.type_names
                .insert(type_name.type_id, to_pascal_case(name));
        }

        self.assign_generated_names();

        self.mark_node_reachable();

        if self.config.emit_node_interface && self.node_reachable {
            self.emit_node_interface();
        }
    }

    pub(super) fn emit_supporting_type(&mut self, type_id: TypeId) {
        if self.emitted_types.contains(&type_id) {
            return;
        }

        let Some(type_def) = self.types.get(type_id) else {
            return;
        };

        match type_def.decode() {
            TypeDefKind::Primitive(_) => (),
            TypeDefKind::Wrapper {
                kind: TypeKind::Alias,
                ..
            } => {
                if let Some(name) = self.type_names.get(&type_id).cloned() {
                    self.emit_node_alias(&name);
                    self.emitted_types.insert(type_id);
                }
            }
            TypeDefKind::Wrapper { .. } => (),
            TypeDefKind::Struct { .. } => {
                if let Some(name) = self.type_names.get(&type_id).cloned() {
                    self.emitted_types.insert(type_id);
                    self.emit_interface(&name, &type_def);
                }
            }
            TypeDefKind::Enum { .. } => {
                if let Some(name) = self.type_names.get(&type_id).cloned() {
                    self.emitted_types.insert(type_id);
                    self.emit_enum(&name, &type_def);
                }
            }
        }
    }

    pub(super) fn emit_type_definition(&mut self, name: &str, type_id: TypeId) {
        self.emitted_types.insert(type_id);
        let type_name = to_pascal_case(name);

        let Some(type_def) = self.types.get(type_id) else {
            let ts_type = self.render_ty(type_id);
            self.emit_type_decl(&type_name, &ts_type);
            return;
        };

        match type_def.decode() {
            TypeDefKind::Struct { .. } => self.emit_interface(&type_name, &type_def),
            TypeDefKind::Enum { .. } => self.emit_enum(&type_name, &type_def),
            _ => {
                let ts_type = self.render_ty(type_id);
                self.emit_type_decl(&type_name, &ts_type);
            }
        }
    }

    /// Emit `export type Name = Body;` with proper coloring.
    pub(super) fn emit_type_decl(&mut self, name: &str, body: &str) {
        let c = self.colors();
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
        let c = self.colors();

        if self.config.export {
            self.output
                .push_str(&format!("{}export{} ", c.dim, c.reset));
        }
        self.output.push_str(&format!(
            "{}interface{} {}{}{} {}{{\n",
            c.dim, c.reset, c.blue, name, c.reset, c.dim
        ));

        let mut fields: Vec<(String, TypeId)> = self
            .types
            .members_of(type_def)
            .map(|member| (self.strings.get(member.name_id).to_string(), member.type_id))
            .collect();
        fields.sort_by(|a, b| a.0.cmp(&b.0));

        for (field_name, field_type) in fields {
            // Every declared field is always present in the output. An optional
            // field renders as `T | null` (the materializer emits null when it does
            // not match), not `T?` which would wrongly permit an absent key.
            let ts_type = self.render_ty(field_type);
            self.output.push_str(&format!(
                "{}  {}{}:{} {}{};\n",
                c.reset, field_name, c.dim, c.reset, ts_type, c.dim
            ));
        }

        self.output.push_str(&format!("{}}}{}\n\n", c.dim, c.reset));
    }

    fn emit_enum(&mut self, name: &str, type_def: &TypeDef) {
        let c = self.colors();
        let mut variant_types = Vec::new();

        for member in self.types.members_of(type_def) {
            let variant_name = self.strings.get(member.name_id);
            let variant_type_name = format!("{}{}", name, to_pascal_case(variant_name));
            variant_types.push(variant_type_name.clone());

            let is_void = self.is_void_type(member.type_id);

            if self.config.export {
                self.output
                    .push_str(&format!("{}export{} ", c.dim, c.reset));
            }
            self.output.push_str(&format!(
                "{}interface{} {}{}{} {}{{\n",
                c.dim, c.reset, c.blue, variant_type_name, c.reset, c.dim
            ));
            self.output.push_str(&format!(
                "{}  $tag{}:{} {}\"{}\"{}{};{}\n",
                c.reset, c.dim, c.reset, c.green, variant_name, c.reset, c.dim, c.reset
            ));
            if !is_void {
                let data_str = self.inline_variant_payload(member.type_id);
                self.output.push_str(&format!(
                    "  $data{}:{} {}{};\n",
                    c.dim, c.reset, data_str, c.dim
                ));
            }
            self.output.push_str(&format!("{}}}{}\n\n", c.dim, c.reset));
        }

        let union = variant_types
            .iter()
            .map(|v| format!("{}{}{}", c.blue, v, c.reset))
            .collect::<Vec<_>>()
            .join(&format!(" {}|{} ", c.dim, c.reset));
        self.emit_type_decl(name, &union);
    }

    fn emit_node_alias(&mut self, name: &str) {
        self.emit_type_decl(name, "Node");
    }

    pub(super) fn emit_type_alias(&mut self, alias_name: &str, target_name: &str) {
        let c = self.colors();
        self.emit_type_decl(alias_name, &format!("{}{}{}", c.blue, target_name, c.reset));
    }

    fn emit_node_interface(&mut self) {
        let c = self.colors();

        if self.config.export {
            self.output
                .push_str(&format!("{}export{} ", c.dim, c.reset));
        }
        self.output.push_str(&format!(
            "{}interface{} {}Node{} {}{{\n",
            c.dim, c.reset, c.blue, c.reset, c.dim
        ));

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
