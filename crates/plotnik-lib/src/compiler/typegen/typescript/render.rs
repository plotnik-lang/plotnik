//! TypeScript declaration rendering over the shared semantic sink.

use crate::bytecode::{TypeDef, TypeDefKind, TypeId, TypeKind};
use crate::compiler::srcgen::sink::{Sink, Style};

use super::Emitter;
use super::emitter::SemanticTag;

impl Emitter<'_> {
    /// Emit the declaration for a named type: `interface` for a struct, a
    /// multi-line tagged union for an enum, `type Name = …` for aliases.
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
                self.emit_type_decl(&name, type_id, body);
            }
            _ => {
                let body = self.render_shape(type_id);
                self.emit_type_decl(&name, type_id, body);
            }
        }
    }

    /// Emit `export type Name = Body;`, retaining style and mapping metadata.
    pub(super) fn emit_type_decl(&mut self, name: &str, type_id: TypeId, body: Sink<SemanticTag>) {
        emit_export(&mut self.sink, self.config.export);
        self.sink.styled(Style::Dim, "type");
        self.sink.push(" ");
        self.sink.set_style(Style::Blue);
        self.push_mapped(name, type_id, None);
        self.sink.reset_style();
        self.sink.push(" ");
        self.sink.styled(Style::Dim, "=");
        self.sink.push(" ");
        self.sink.append(body);
        self.sink.set_style(Style::Dim);
        self.sink.push(";\n\n");
        self.sink.reset_style();
    }

    fn emit_interface(&mut self, name: &str, type_id: TypeId, type_def: &TypeDef) {
        emit_export(&mut self.sink, self.config.export);
        self.sink.styled(Style::Dim, "interface");
        self.sink.push(" ");
        self.sink.set_style(Style::Blue);
        self.push_mapped(name, type_id, None);
        self.sink.reset_style();
        self.sink.push(" ");
        self.sink.set_style(Style::Dim);
        self.sink.push("{\n");

        let mut fields: Vec<(String, TypeId, u16)> = self
            .members_of_with_indices(type_def)
            .map(|(index, member)| {
                (
                    self.strings.get(member.name_id).to_string(),
                    member.type_id,
                    index,
                )
            })
            .collect();
        fields.sort_by(|left, right| left.0.cmp(&right.0));

        for (name, field_type, member) in fields {
            // Optional fields remain present and render as `T | null`, not `T?`.
            let rendered = self.render_ty(field_type);
            self.sink.reset_style();
            self.sink.push("  ");
            self.push_mapped(&name, type_id, Some(member));
            self.sink.set_style(Style::Dim);
            self.sink.push(":");
            self.sink.reset_style();
            self.sink.push(" ");
            self.sink.append(rendered);
            self.sink.set_style(Style::Dim);
            self.sink.push(";\n");
        }

        self.sink.set_style(Style::Dim);
        self.sink.push("}");
        self.sink.reset_style();
        self.sink.push("\n\n");
    }

    /// Emit an enum as one multi-line union of inline variants.
    fn emit_enum(&mut self, name: &str, type_id: TypeId, type_def: &TypeDef) {
        emit_export(&mut self.sink, self.config.export);
        self.sink.styled(Style::Dim, "type");
        self.sink.push(" ");
        self.sink.set_style(Style::Blue);
        self.push_mapped(name, type_id, None);
        self.sink.reset_style();
        self.sink.push(" ");
        self.sink.styled(Style::Dim, "=");
        self.sink.push("\n");

        let variants: Vec<(String, TypeId, u16)> = self
            .members_of_with_indices(type_def)
            .map(|(index, member)| {
                (
                    self.strings.get(member.name_id).to_string(),
                    member.type_id,
                    index,
                )
            })
            .collect();
        let last = variants.len().saturating_sub(1);
        for (position, (name, payload, member)) in variants.into_iter().enumerate() {
            let rendered = self.render_variant(
                &name,
                payload,
                Some(SemanticTag {
                    type_id,
                    member: Some(member),
                }),
                true,
            );
            self.sink.push("  ");
            self.sink.styled(Style::Dim, "|");
            self.sink.push(" ");
            self.sink.append(rendered);
            self.sink.set_style(Style::Dim);
            if position == last {
                self.sink.push(";");
            }
            self.sink.reset_style();
            self.sink.push("\n");
        }
        self.sink.push("\n");
    }

    pub(super) fn emit_node_interface(&mut self) {
        emit_export(&mut self.sink, self.config.export);
        self.sink.styled(Style::Dim, "interface");
        self.sink.push(" ");
        self.sink.styled(Style::Blue, "Node");
        self.sink.push(" ");
        self.sink.set_style(Style::Dim);
        self.sink.push("{\n");

        emit_node_field(&mut self.sink, "kind", text("string"));
        emit_node_field(&mut self.sink, "text", text("string"));

        self.sink.reset_style();
        self.sink.push("  span");
        self.sink.styled(Style::Dim, ":");
        self.sink.push(" ");
        self.sink.styled(Style::Dim, "[");
        self.sink.push("number");
        self.sink.styled(Style::Dim, ", ");
        self.sink.push("number");
        self.sink.set_style(Style::Dim);
        self.sink.push("]");
        self.sink.set_style(Style::Dim);
        self.sink.push(";\n");

        if self.config.verbose_nodes {
            emit_node_field(&mut self.sink, "startPosition", position_type());
            emit_node_field(&mut self.sink, "endPosition", position_type());
        }

        self.sink.set_style(Style::Dim);
        self.sink.push("}");
        self.sink.reset_style();
        self.sink.push("\n\n");
    }
}

fn emit_export(sink: &mut Sink<SemanticTag>, enabled: bool) {
    if !enabled {
        return;
    }
    sink.styled(Style::Dim, "export");
    sink.push(" ");
}

fn emit_node_field(sink: &mut Sink<SemanticTag>, name: &str, ty: Sink<SemanticTag>) {
    sink.reset_style();
    sink.push("  ");
    sink.push(name);
    sink.styled(Style::Dim, ":");
    sink.push(" ");
    sink.append(ty);
    sink.set_style(Style::Dim);
    sink.push(";\n");
}

fn position_type() -> Sink<SemanticTag> {
    let mut out = Sink::new();
    out.styled(Style::Dim, "{");
    out.push(" row");
    out.styled(Style::Dim, ":");
    out.push(" number");
    out.set_style(Style::Dim);
    out.push("; column");
    out.set_style(Style::Dim);
    out.push(":");
    out.reset_style();
    out.push(" number");
    out.set_style(Style::Dim);
    out.push("; ");
    out.set_style(Style::Dim);
    out.push("}");
    out
}

fn text(value: &str) -> Sink<SemanticTag> {
    let mut out = Sink::new();
    out.push(value);
    out
}
