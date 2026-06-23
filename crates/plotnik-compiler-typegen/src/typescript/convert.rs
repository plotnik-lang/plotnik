//! Type to TypeScript string conversion.

use plotnik_bytecode::{TypeDef, TypeDefKind, TypeId, TypeKind};

use super::Emitter;
use super::config::VoidType;

impl Emitter<'_> {
    pub(super) fn render_ty(&self, type_id: TypeId) -> String {
        let c = self.colors();
        let Some(type_def) = self.types.get(type_id) else {
            return "unknown".to_string();
        };

        match type_def.decode() {
            TypeDefKind::Primitive(TypeKind::Void) => match self.config.void_type {
                VoidType::Undefined => "undefined".to_string(),
                VoidType::Null => "null".to_string(),
            },
            TypeDefKind::Primitive(TypeKind::Node) => "Node".to_string(),
            TypeDefKind::Primitive(_) => "unknown".to_string(),
            TypeDefKind::Wrapper {
                kind: TypeKind::Alias,
                ..
            } => {
                if let Some(name) = self.type_names.get(&type_id) {
                    format!("{}{}{}", c.blue, name, c.reset)
                } else {
                    "Node".to_string()
                }
            }
            TypeDefKind::Wrapper {
                kind: TypeKind::ArrayZeroOrMore,
                inner,
            } => {
                let elem_type = self.render_ty(inner);
                format!("{}{}[]{}", elem_type, c.dim, c.reset)
            }
            TypeDefKind::Wrapper {
                kind: TypeKind::ArrayOneOrMore,
                inner,
            } => {
                let elem_type = self.render_ty(inner);
                format!(
                    "{}[{}{}{}, ...{}{}{}[]]{}",
                    c.dim, c.reset, elem_type, c.dim, c.reset, elem_type, c.dim, c.reset
                )
            }
            TypeDefKind::Wrapper {
                kind: TypeKind::Optional,
                inner,
            } => {
                let inner_type = self.render_ty(inner);
                format!("{} {}|{} null", inner_type, c.dim, c.reset)
            }
            TypeDefKind::Wrapper { .. } => "unknown".to_string(),
            TypeDefKind::Struct { .. } => {
                if let Some(name) = self.type_names.get(&type_id) {
                    format!("{}{}{}", c.blue, name, c.reset)
                } else {
                    self.inline_struct(&type_def)
                }
            }
            TypeDefKind::Enum { .. } => {
                if let Some(name) = self.type_names.get(&type_id) {
                    format!("{}{}{}", c.blue, name, c.reset)
                } else {
                    self.inline_enum(&type_def)
                }
            }
        }
    }

    pub(super) fn inline_struct(&self, type_def: &TypeDef) -> String {
        let c = self.colors();
        let member_count = match type_def.decode() {
            TypeDefKind::Struct { member_count, .. } => member_count,
            _ => 0,
        };
        if member_count == 0 {
            return format!("{}{{}}{}", c.dim, c.reset);
        }

        let mut fields: Vec<(String, TypeId)> = self
            .types
            .members_of(type_def)
            .map(|member| (self.strings.get(member.name_id).to_string(), member.type_id))
            .collect();
        fields.sort_by(|a, b| a.0.cmp(&b.0));

        // Optional fields render as `T | null` (always present), matching the
        // materializer — see `emit_interface`.
        let field_strs: Vec<String> = fields
            .iter()
            .map(|(name, ty)| {
                let ts_type = self.render_ty(*ty);
                format!("{}{}:{} {}", name, c.dim, c.reset, ts_type)
            })
            .collect();

        format!(
            "{}{{{} {} {}}}{}",
            c.dim,
            c.reset,
            field_strs.join(&format!("{}; ", c.dim)),
            c.dim,
            c.reset
        )
    }

    fn inline_enum(&self, type_def: &TypeDef) -> String {
        let c = self.colors();
        let variant_strs: Vec<String> = self
            .types
            .members_of(type_def)
            .map(|member| {
                let name = self.strings.get(member.name_id);
                if self.is_void_type(member.type_id) {
                    // Void payload: omit $data
                    format!(
                        "{}{{{} $tag{}:{} {}\"{}\"{}{}}}{}",
                        c.dim, c.reset, c.dim, c.reset, c.green, name, c.reset, c.dim, c.reset
                    )
                } else {
                    let data_type = self.render_ty(member.type_id);
                    format!(
                        "{}{{{} $tag{}:{} {}\"{}\"{}{}; $data{}:{} {} {}}}{}",
                        c.dim,
                        c.reset,
                        c.dim,
                        c.reset,
                        c.green,
                        name,
                        c.reset,
                        c.dim,
                        c.dim,
                        c.reset,
                        data_type,
                        c.dim,
                        c.reset
                    )
                }
            })
            .collect();

        variant_strs.join(&format!(" {}|{} ", c.dim, c.reset))
    }

    pub(super) fn inline_variant_payload(&self, type_id: TypeId) -> String {
        let c = self.colors();
        let Some(type_def) = self.types.get(type_id) else {
            return self.render_ty(type_id);
        };

        match type_def.decode() {
            TypeDefKind::Primitive(TypeKind::Void) => format!("{}{{}}{}", c.dim, c.reset),
            TypeDefKind::Struct { .. } => self.inline_struct(&type_def),
            _ => self.render_ty(type_id),
        }
    }

    pub(super) fn is_void_type(&self, type_id: TypeId) -> bool {
        self.types
            .get(type_id)
            .is_some_and(|def| matches!(def.decode(), TypeDefKind::Primitive(TypeKind::Void)))
    }
}
