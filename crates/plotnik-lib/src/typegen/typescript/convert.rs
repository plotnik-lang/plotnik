//! Type to TypeScript string conversion.

use crate::bytecode::{TypeDef, TypeId, TypeKind};

use super::Emitter;
use super::config::VoidType;

impl Emitter<'_> {
    pub(super) fn type_to_ts(&self, type_id: TypeId) -> String {
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
                let elem_type = self.type_to_ts(TypeId(type_def.data));
                format!("{}{}[]{}", elem_type, c.dim, c.reset)
            }
            TypeKind::ArrayOneOrMore => {
                let elem_type = self.type_to_ts(TypeId(type_def.data));
                format!(
                    "{}[{}{}{}, ...{}{}{}[]]{}",
                    c.dim, c.reset, elem_type, c.dim, c.reset, elem_type, c.dim, c.reset
                )
            }
            TypeKind::Optional => {
                let inner_type = self.type_to_ts(TypeId(type_def.data));
                format!("{} {}|{} null", inner_type, c.dim, c.reset)
            }
        }
    }

    fn inline_composite(&self, _type_id: TypeId, type_def: &TypeDef, kind: &TypeKind) -> String {
        match kind {
            TypeKind::Struct => self.inline_struct(type_def),
            TypeKind::Enum => self.inline_enum(type_def),
            _ => "unknown".to_string(),
        }
    }

    pub(super) fn inline_struct(&self, type_def: &TypeDef) -> String {
        let c = self.c();
        if type_def.count == 0 {
            return format!("{}{{}}{}", c.dim, c.reset);
        }

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
                let name = self.strings.get(member.name);
                if self.is_void_type(member.type_id) {
                    // Void payload: omit $data
                    format!(
                        "{}{{{} $tag{}:{} {}\"{}\"{}{}}}{}",
                        c.dim, c.reset, c.dim, c.reset, c.green, name, c.reset, c.dim, c.reset
                    )
                } else {
                    let data_type = self.type_to_ts(member.type_id);
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

    pub(super) fn is_void_type(&self, type_id: TypeId) -> bool {
        self.types
            .get(type_id)
            .and_then(|def| def.type_kind())
            .is_some_and(|k| k == TypeKind::Void)
    }
}
