//! Type-to-TypeScript fragments.
//!
//! Fragments retain ANSI style events and optional semantic member ranges, so
//! declarations, inline enum payloads, plain output, and mapped output all use
//! this one rendering path.

use crate::bytecode::{TypeDef, TypeDefKind, TypeId, TypeKind};
use crate::compiler::codegen::emit::sink::{Sink, Style};

use super::Emitter;
use super::config::VoidType;
use super::emitter::SemanticTag;

#[derive(Clone, Copy)]
enum MemberTags {
    None,
    Inline,
}

impl Emitter<'_> {
    /// Render a type reference at a use site: any named type renders by its
    /// name; anonymous types render their shape inline.
    pub(super) fn render_ty(&self, type_id: TypeId) -> Sink<SemanticTag> {
        let Some(type_def) = self.types.get(type_id) else {
            return text("unknown");
        };
        if !matches!(type_def.decode(), TypeDefKind::Primitive(_))
            && let Some(name) = self.type_names.get(&type_id)
        {
            let mut out = Sink::new();
            out.styled(Style::Blue, name);
            return out;
        }
        self.render_shape(type_id)
    }

    /// Render a declaration body structurally, while nested positions still
    /// use their nominal names through [`Self::render_ty`].
    pub(super) fn render_shape(&self, type_id: TypeId) -> Sink<SemanticTag> {
        let Some(type_def) = self.types.get(type_id) else {
            return text("unknown");
        };

        match type_def.decode() {
            TypeDefKind::Primitive(TypeKind::Void) => match self.config.void_type {
                VoidType::Undefined => text("undefined"),
                VoidType::Null => text("null"),
            },
            TypeDefKind::Primitive(TypeKind::Node) => text("Node"),
            TypeDefKind::Primitive(_) => text("unknown"),
            TypeDefKind::Wrapper {
                kind: TypeKind::Alias,
                inner,
            } => self.render_ty(inner),
            TypeDefKind::Wrapper {
                kind: TypeKind::ArrayZeroOrMore,
                inner,
            } => {
                let mut out = self.render_ty(inner);
                out.styled(Style::Dim, "[]");
                out
            }
            TypeDefKind::Wrapper {
                kind: TypeKind::ArrayOneOrMore,
                inner,
            } => {
                let element = self.render_ty(inner);
                let mut out = Sink::new();
                out.styled(Style::Dim, "[");
                out.append(element);
                out.styled(Style::Dim, ", ...");
                out.append(self.render_ty(inner));
                out.styled(Style::Dim, "[]]");
                out
            }
            TypeDefKind::Wrapper {
                kind: TypeKind::Optional,
                inner,
            } => {
                let mut out = self.render_ty(inner);
                out.push(" ");
                out.styled(Style::Dim, "|");
                out.push(" null");
                out
            }
            TypeDefKind::Wrapper { .. } => text("unknown"),
            TypeDefKind::Struct { .. } => self.inline_struct(&type_def, type_id, MemberTags::None),
            TypeDefKind::Enum { .. } => self.inline_enum(&type_def),
        }
    }

    fn inline_struct(
        &self,
        type_def: &TypeDef,
        type_id: TypeId,
        tags: MemberTags,
    ) -> Sink<SemanticTag> {
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
        if fields.is_empty() {
            let mut out = Sink::new();
            out.styled(Style::Dim, "{}");
            return out;
        }
        fields.sort_by(|left, right| left.0.cmp(&right.0));

        let mut out = Sink::new();
        out.styled(Style::Dim, "{");
        out.push(" ");
        let last = fields.len() - 1;
        for (position, (name, field_type, member)) in fields.into_iter().enumerate() {
            match tags {
                MemberTags::None => out.push(&name),
                MemberTags::Inline => out.tagged(
                    SemanticTag {
                        type_id,
                        member: Some(member),
                    },
                    |out| out.push(&name),
                ),
            }
            out.set_style(Style::Dim);
            out.push(":");
            out.reset_style();
            out.push(" ");
            out.append(self.render_ty(field_type));
            if position != last {
                // Deliberately no reset: this is byte-identical to the old
                // renderer, whose next field name inherits dim until `:`.
                out.set_style(Style::Dim);
                out.push("; ");
            }
        }
        out.push(" ");
        out.styled(Style::Dim, "}");
        out
    }

    fn inline_enum(&self, type_def: &TypeDef) -> Sink<SemanticTag> {
        let variants: Vec<(String, TypeId)> = self
            .types
            .members_of(type_def)
            .map(|member| (self.strings.get(member.name_id).to_string(), member.type_id))
            .collect();
        let mut out = Sink::new();
        for (position, (name, payload)) in variants.iter().enumerate() {
            if position > 0 {
                out.push(" ");
                out.styled(Style::Dim, "|");
                out.push(" ");
            }
            out.append(self.render_variant(name, *payload, None, false));
        }
        out
    }

    /// One variant literal, optionally tagging the variant and inline payload
    /// fields for mapped d.ts output.
    pub(super) fn render_variant(
        &self,
        name: &str,
        payload_type: TypeId,
        variant: Option<SemanticTag>,
        payload_tags: bool,
    ) -> Sink<SemanticTag> {
        let mut out = Sink::new();
        out.styled(Style::Dim, "{");
        out.push(" $tag");
        out.styled(Style::Dim, ":");
        out.push(" ");
        out.set_style(Style::Green);
        out.push("\"");
        match variant {
            Some(tag) => out.tagged(tag, |out| out.push(name)),
            None => out.push(name),
        }
        out.push("\"");
        out.reset_style();

        if self.is_void_type(payload_type) {
            out.push(" ");
            out.styled(Style::Dim, "}");
            return out;
        }

        // Keep both dim events: the prior renderer emitted one before
        // `; $data` and another before `:`.
        out.set_style(Style::Dim);
        out.push("; $data");
        out.set_style(Style::Dim);
        out.push(":");
        out.reset_style();
        out.push(" ");
        out.append(self.inline_variant_payload(
            payload_type,
            if payload_tags {
                MemberTags::Inline
            } else {
                MemberTags::None
            },
        ));
        out.push(" ");
        out.styled(Style::Dim, "}");
        out
    }

    fn inline_variant_payload(&self, type_id: TypeId, tags: MemberTags) -> Sink<SemanticTag> {
        let Some(type_def) = self.types.get(type_id) else {
            return self.render_ty(type_id);
        };

        if matches!(type_def.decode(), TypeDefKind::Struct { .. }) {
            return self.inline_struct(&type_def, type_id, tags);
        }
        self.render_ty(type_id)
    }

    pub(super) fn is_void_type(&self, type_id: TypeId) -> bool {
        self.types
            .get(type_id)
            .is_some_and(|def| matches!(def.decode(), TypeDefKind::Primitive(TypeKind::Void)))
    }
}

fn text(value: &str) -> Sink<SemanticTag> {
    let mut out = Sink::new();
    out.push(value);
    out
}
