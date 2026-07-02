//! Output rendering methods.

use crate::bytecode::{TypeDef, TypeDefKind, TypeId, TypeKind};

use super::Emitter;

impl Emitter<'_> {
    /// Emit the declaration for a named type: `interface` for a struct, a
    /// multi-line tagged union for an enum, `type Name = …` for aliases.
    /// Unnamed composites emit nothing — they render inline at use sites.
    pub(super) fn emit_declaration(&mut self, type_id: TypeId) {
        if self.emitted_types.contains(&type_id) {
            return;
        }

        let Some(type_def) = self.types.get(type_id) else {
            return;
        };
        let Some(name) = self.type_names.get(&type_id).cloned() else {
            return;
        };

        self.emitted_types.insert(type_id);
        if !self.declared_names.insert(name.clone()) {
            // A nominal twin: the compiler guarantees same name ⇒ same shape,
            // so the first declaration serves this id too.
            return;
        }

        match type_def.decode() {
            TypeDefKind::Struct { .. } => self.emit_interface(&name, &type_def),
            TypeDefKind::Enum { .. } => self.emit_enum(&name, &type_def),
            TypeDefKind::Wrapper {
                kind: TypeKind::Alias,
                inner,
            } => {
                let body = self.render_ty(inner);
                self.emit_type_decl(&name, &body);
            }
            _ => {
                let body = self.render_shape(type_id);
                self.emit_type_decl(&name, &body);
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

    /// Emit an enum as one multi-line union of inline variants:
    ///
    /// ```ts
    /// export type Expr =
    ///   | { $tag: "Lit"; $data: { value: Node } }
    ///   | { $tag: "Neg"; $data: { inner: Expr } };
    /// ```
    ///
    /// Variants have no standalone declarations; a void variant omits `$data`.
    fn emit_enum(&mut self, name: &str, type_def: &TypeDef) {
        let c = self.colors();

        if self.config.export {
            self.output
                .push_str(&format!("{}export{} ", c.dim, c.reset));
        }
        self.output.push_str(&format!(
            "{}type{} {}{}{} {}={}\n",
            c.dim, c.reset, c.blue, name, c.reset, c.dim, c.reset
        ));

        let variants: Vec<String> = self
            .types
            .members_of(type_def)
            .map(|member| {
                let variant_name = self.strings.get(member.name_id).to_string();
                self.render_variant(&variant_name, member.type_id)
            })
            .collect();

        let last = variants.len().saturating_sub(1);
        for (i, variant) in variants.into_iter().enumerate() {
            let terminator = if i == last { ";" } else { "" };
            self.output.push_str(&format!(
                "  {}|{} {}{}{}{}\n",
                c.dim, c.reset, variant, c.dim, terminator, c.reset
            ));
        }
        self.output.push('\n');
    }

    pub(super) fn emit_node_interface(&mut self) {
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
