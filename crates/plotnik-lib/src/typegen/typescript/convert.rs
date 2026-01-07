//! Type to TypeScript string conversion.

use crate::bytecode::{TypeData, TypeDef, TypeId, TypeKind};

use super::Emitter;
use super::config::VoidType;

impl Emitter<'_> {
    pub(super) fn type_to_ts(&self, type_id: TypeId) -> String {
        let c = self.c();
        let Some(type_def) = self.types.get(type_id) else {
            return "unknown".to_string();
        };

        match type_def.classify() {
            TypeData::Primitive(TypeKind::Void) => match self.config.void_type {
                VoidType::Undefined => "undefined".to_string(),
                VoidType::Null => "null".to_string(),
            },
            TypeData::Primitive(TypeKind::Node) => "Node".to_string(),
            TypeData::Primitive(TypeKind::String) => "string".to_string(),
            TypeData::Primitive(_) => "unknown".to_string(),
            TypeData::Wrapper {
                kind: TypeKind::Alias,
                ..
            } => {
                if let Some(name) = self.type_names.get(&type_id) {
                    format!("{}{}{}", c.blue, name, c.reset)
                } else {
                    "Node".to_string()
                }
            }
            TypeData::Wrapper {
                kind: TypeKind::ArrayZeroOrMore,
                inner,
            } => {
                let elem_type = self.type_to_ts(inner);
                format!("{}{}[]{}", elem_type, c.dim, c.reset)
            }
            TypeData::Wrapper {
                kind: TypeKind::ArrayOneOrMore,
                inner,
            } => {
                let elem_type = self.type_to_ts(inner);
                format!(
                    "{}[{}{}{}, ...{}{}{}[]]{}",
                    c.dim, c.reset, elem_type, c.dim, c.reset, elem_type, c.dim, c.reset
                )
            }
            TypeData::Wrapper {
                kind: TypeKind::Optional,
                inner,
            } => {
                let inner_type = self.type_to_ts(inner);
                format!("{} {}|{} null", inner_type, c.dim, c.reset)
            }
            TypeData::Wrapper { .. } => "unknown".to_string(),
            TypeData::Composite { kind, .. } => {
                if let Some(name) = self.type_names.get(&type_id) {
                    format!("{}{}{}", c.blue, name, c.reset)
                } else {
                    self.inline_composite(&type_def, kind)
                }
            }
        }
    }

    fn inline_composite(&self, type_def: &TypeDef, kind: TypeKind) -> String {
        match kind {
            TypeKind::Struct => self.inline_struct(type_def),
            TypeKind::Enum => self.inline_enum(type_def),
            _ => "unknown".to_string(),
        }
    }

    pub(super) fn inline_struct(&self, type_def: &TypeDef) -> String {
        let c = self.c();
        let member_count = match type_def.classify() {
            TypeData::Composite { member_count, .. } => member_count,
            _ => 0,
        };
        if member_count == 0 {
            return format!("{}{{}}{}", c.dim, c.reset);
        }

        let mut fields: Vec<(String, TypeId, bool)> = self
            .types
            .members_of(type_def)
            .map(|member| {
                let field_name = self.strings.get(member.name()).to_string();
                let (inner_type, optional) = self.types.unwrap_optional(member.type_id());
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
            "{}{{{} {} {}}}{}",
            c.dim,
            c.reset,
            field_strs.join(&format!("{}; ", c.dim)),
            c.dim,
            c.reset
        )
    }

    fn inline_enum(&self, type_def: &TypeDef) -> String {
        let c = self.c();
        let variant_strs: Vec<String> = self
            .types
            .members_of(type_def)
            .map(|member| {
                let name = self.strings.get(member.name());
                if self.is_void_type(member.type_id()) {
                    // Void payload: omit $data
                    format!(
                        "{}{{{} $tag{}:{} {}\"{}\"{}{}}}{}",
                        c.dim, c.reset, c.dim, c.reset, c.green, name, c.reset, c.dim, c.reset
                    )
                } else {
                    let data_type = self.type_to_ts(member.type_id());
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

    pub(super) fn inline_data_type(&self, type_id: TypeId) -> String {
        let c = self.c();
        let Some(type_def) = self.types.get(type_id) else {
            return self.type_to_ts(type_id);
        };

        match type_def.classify() {
            TypeData::Primitive(TypeKind::Void) => format!("{}{{}}{}", c.dim, c.reset),
            TypeData::Composite {
                kind: TypeKind::Struct,
                ..
            } => self.inline_struct(&type_def),
            _ => self.type_to_ts(type_id),
        }
    }

    pub(super) fn is_void_type(&self, type_id: TypeId) -> bool {
        self.types
            .get(type_id)
            .is_some_and(|def| matches!(def.classify(), TypeData::Primitive(TypeKind::Void)))
    }
}
