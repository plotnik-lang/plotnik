//! Name generation for anonymous types.

use std::collections::HashMap;

use crate::core::utils::to_pascal_case;

use crate::bytecode::{TypeDefKind, TypeId, TypeKind};

use super::Emitter;

#[derive(Clone, Debug)]
pub(super) struct NameHint {
    pub entry_name: String,
    pub member_name: Option<String>,
}

impl Emitter<'_> {
    pub(super) fn assign_generated_names(&mut self) {
        let mut contexts: HashMap<TypeId, NameHint> = HashMap::new();

        for i in 0..self.entrypoints.len() {
            let ep = self.entrypoints.get(i);
            let entry_name = self.strings.get(ep.name());
            self.collect_naming_contexts(
                ep.result_type(),
                &NameHint {
                    entry_name: entry_name.to_string(),
                    member_name: None,
                },
                &mut contexts,
            );
        }

        for i in 0..self.types.defs_count() {
            let type_id = TypeId(i as u16);
            if self.type_names.contains_key(&type_id) {
                continue;
            }

            let type_def = self.types.def(i);
            if !self.is_named_struct_or_enum(&type_def) {
                continue;
            }

            let name = if let Some(ctx) = contexts.get(&type_id) {
                self.contextual_name(ctx)
            } else {
                self.fallback_name(&type_def)
            };
            self.type_names.insert(type_id, name);
        }
    }

    fn collect_naming_contexts(
        &self,
        type_id: TypeId,
        ctx: &NameHint,
        contexts: &mut HashMap<TypeId, NameHint>,
    ) {
        if contexts.contains_key(&type_id) {
            return;
        }

        let Some(type_def) = self.types.get(type_id) else {
            return;
        };

        match type_def.decode() {
            TypeDefKind::Primitive(_) => {}
            TypeDefKind::Wrapper {
                kind: TypeKind::Alias,
                ..
            } => {}
            TypeDefKind::Wrapper { inner, .. } => {
                self.collect_naming_contexts(inner, ctx, contexts);
            }
            TypeDefKind::Struct { .. } => {
                contexts.entry(type_id).or_insert_with(|| ctx.clone());
                for member in self.types.members_of(&type_def) {
                    let member_name = self.strings.get(member.name_id);
                    let (inner_type, _) = self.types.unwrap_optional(member.type_id);
                    let field_ctx = NameHint {
                        entry_name: ctx.entry_name.clone(),
                        member_name: Some(member_name.to_string()),
                    };
                    self.collect_naming_contexts(inner_type, &field_ctx, contexts);
                }
            }
            TypeDefKind::Enum { .. } => {
                contexts.entry(type_id).or_insert_with(|| ctx.clone());
            }
        }
    }

    pub(super) fn is_named_struct_or_enum(&self, type_def: &crate::bytecode::TypeDef) -> bool {
        matches!(
            type_def.decode(),
            TypeDefKind::Struct { .. } | TypeDefKind::Enum { .. }
        )
    }

    pub(super) fn contextual_name(&mut self, ctx: &NameHint) -> String {
        let base = if let Some(field) = &ctx.member_name {
            format!(
                "{}{}",
                to_pascal_case(&ctx.entry_name),
                to_pascal_case(field)
            )
        } else {
            to_pascal_case(&ctx.entry_name)
        };
        self.unique_name(&base)
    }

    pub(super) fn fallback_name(&mut self, type_def: &crate::bytecode::TypeDef) -> String {
        let base = match type_def.decode() {
            TypeDefKind::Struct { .. } => "Struct",
            TypeDefKind::Enum { .. } => "Enum",
            _ => "Type",
        };
        self.unique_name(base)
    }

    pub(super) fn unique_name(&mut self, base: &str) -> String {
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
