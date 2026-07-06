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
            TypeDefKind::Struct { .. } => self.emit_interface(&name, type_id, &type_def),
            TypeDefKind::Enum { .. } => self.emit_enum(&name, type_id, &type_def),
            TypeDefKind::Wrapper {
                kind: TypeKind::Alias,
                inner,
            } => {
                let body = self.render_ty(inner);
                self.emit_type_decl(&name, type_id, &body);
            }
            _ => {
                let body = self.render_shape(type_id);
                self.emit_type_decl(&name, type_id, &body);
            }
        }
    }

    /// Emit `export type Name = Body;` with proper coloring.
    pub(super) fn emit_type_decl(&mut self, name: &str, type_id: TypeId, body: &str) {
        let c = self.colors();
        if self.config.export {
            self.output
                .push_str(&format!("{}export{} ", c.dim, c.reset));
        }
        self.output
            .push_str(&format!("{}type{} {}", c.dim, c.reset, c.blue));
        self.push_mapped(name, type_id, None);
        self.output.push_str(&format!(
            "{} {}={} {}{};\n\n",
            c.reset, c.dim, c.reset, body, c.dim
        ));
        self.output.push_str(c.reset);
    }

    fn emit_interface(&mut self, name: &str, type_id: TypeId, type_def: &TypeDef) {
        let c = self.colors();

        if self.config.export {
            self.output
                .push_str(&format!("{}export{} ", c.dim, c.reset));
        }
        self.output
            .push_str(&format!("{}interface{} {}", c.dim, c.reset, c.blue));
        self.push_mapped(name, type_id, None);
        self.output.push_str(&format!("{} {}{{\n", c.reset, c.dim));

        let mut fields: Vec<(String, TypeId, u16)> = self
            .members_of_with_indices(type_def)
            .map(|(idx, member)| {
                (
                    self.strings.get(member.name_id).to_string(),
                    member.type_id,
                    idx,
                )
            })
            .collect();
        fields.sort_by(|a, b| a.0.cmp(&b.0));

        for (field_name, field_type, member_idx) in fields {
            // Every declared field is always present in the output. An optional
            // field renders as `T | null` (the materializer emits null when it does
            // not match), not `T?` which would wrongly permit an absent key.
            let ts_type = self.render_ty(field_type);
            self.output.push_str(&format!("{}  ", c.reset));
            self.push_mapped(&field_name, type_id, Some(member_idx));
            self.output
                .push_str(&format!("{}:{} {}{};\n", c.dim, c.reset, ts_type, c.dim));
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
    fn emit_enum(&mut self, name: &str, type_id: TypeId, type_def: &TypeDef) {
        let c = self.colors();

        if self.config.export {
            self.output
                .push_str(&format!("{}export{} ", c.dim, c.reset));
        }
        self.output
            .push_str(&format!("{}type{} {}", c.dim, c.reset, c.blue));
        self.push_mapped(name, type_id, None);
        self.output
            .push_str(&format!("{} {}={}\n", c.reset, c.dim, c.reset));

        let variants: Vec<(String, TypeId, u16)> = self
            .members_of_with_indices(type_def)
            .map(|(idx, member)| {
                (
                    self.strings.get(member.name_id).to_string(),
                    member.type_id,
                    idx,
                )
            })
            .collect();

        let last = variants.len().saturating_sub(1);
        for (i, (variant_name, payload_type, member_idx)) in variants.into_iter().enumerate() {
            let terminator = if i == last { ";" } else { "" };
            self.output.push_str(&format!("  {}|{} ", c.dim, c.reset));
            self.emit_variant(&variant_name, payload_type, type_id, member_idx);
            self.output
                .push_str(&format!("{}{}{}\n", c.dim, terminator, c.reset));
        }
        self.output.push('\n');
    }

    /// Emit one variant literal, byte-identical to
    /// [`render_variant`](Self::render_variant), but writing through
    /// `push_mapped` so the tag and payload field ranges are recorded at push
    /// time. Re-finding names in rendered text by substring scan is not sound —
    /// a field name that collides with `$tag:`/`$data:` text binds to the
    /// wrong offset.
    fn emit_variant(
        &mut self,
        variant_name: &str,
        payload_type: TypeId,
        enum_type: TypeId,
        variant_member: u16,
    ) {
        let c = self.colors();
        self.output.push_str(&format!(
            "{}{{{} $tag{}:{} {}\"",
            c.dim, c.reset, c.dim, c.reset, c.green
        ));
        self.push_mapped(variant_name, enum_type, Some(variant_member));
        self.output.push_str(&format!("\"{}", c.reset));

        if self.is_void_type(payload_type) {
            self.output.push_str(&format!(" {}}}{}", c.dim, c.reset));
            return;
        }

        self.output
            .push_str(&format!("{}; $data{}:{} ", c.dim, c.dim, c.reset));
        self.emit_variant_payload(payload_type);
        self.output.push_str(&format!(" {}}}{}", c.dim, c.reset));
    }

    /// Mapped twin of [`inline_variant_payload`](Self::inline_variant_payload):
    /// a struct payload inlines with mapped field names, everything else
    /// renders unmapped.
    fn emit_variant_payload(&mut self, payload_type: TypeId) {
        if let Some(type_def) = self.types.get(payload_type)
            && matches!(type_def.decode(), TypeDefKind::Struct { .. })
        {
            self.emit_inline_struct_mapped(&type_def, payload_type);
            return;
        }
        let rendered = self.render_ty(payload_type);
        self.output.push_str(&rendered);
    }

    /// Mapped twin of [`inline_struct`](Self::inline_struct) — same bytes,
    /// field names recorded as they are written.
    fn emit_inline_struct_mapped(&mut self, type_def: &TypeDef, type_id: TypeId) {
        let c = self.colors();
        let mut fields: Vec<(String, TypeId, u16)> = self
            .members_of_with_indices(type_def)
            .map(|(idx, member)| {
                (
                    self.strings.get(member.name_id).to_string(),
                    member.type_id,
                    idx,
                )
            })
            .collect();
        if fields.is_empty() {
            self.output.push_str(&format!("{}{{}}{}", c.dim, c.reset));
            return;
        }
        fields.sort_by(|a, b| a.0.cmp(&b.0));

        self.output.push_str(&format!("{}{{{} ", c.dim, c.reset));
        let last = fields.len() - 1;
        for (i, (field_name, field_type, member_idx)) in fields.into_iter().enumerate() {
            self.push_mapped(&field_name, type_id, Some(member_idx));
            let ts_type = self.render_ty(field_type);
            self.output
                .push_str(&format!("{}:{} {}", c.dim, c.reset, ts_type));
            if i != last {
                self.output.push_str(&format!("{}; ", c.dim));
            }
        }
        self.output.push_str(&format!(" {}}}{}", c.dim, c.reset));
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
