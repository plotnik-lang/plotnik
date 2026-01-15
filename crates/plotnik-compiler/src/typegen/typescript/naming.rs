//! Name generation for anonymous types.

use std::collections::HashMap;

use plotnik_core::utils::to_pascal_case;

use plotnik_bytecode::{TypeData, TypeId, TypeKind};

use super::Emitter;

#[derive(Clone, Debug)]
pub(super) struct NamingContext {
    pub def_name: String,
    pub field_name: Option<String>,
}

impl Emitter<'_> {
    pub(super) fn assign_generated_names(&mut self) {
        // Collect naming contexts from entrypoints → fields
        let mut contexts: HashMap<TypeId, NamingContext> = HashMap::new();

        for i in 0..self.entrypoints.len() {
            let ep = self.entrypoints.get(i);
            let def_name = self.strings.get(ep.name);
            self.collect_naming_contexts(
                ep.result_type(),
                &NamingContext {
                    def_name: def_name.to_string(),
                    field_name: None,
                },
                &mut contexts,
            );
        }

        // Assign names to types that need them
        for i in 0..self.types.defs_count() {
            let type_id = TypeId(i as u16);
            if self.type_names.contains_key(&type_id) {
                continue;
            }

            let type_def = self.types.get_def(i);
            if !self.needs_generated_name(&type_def) {
                continue;
            }

            let name = if let Some(ctx) = contexts.get(&type_id) {
                self.generate_contextual_name(ctx)
            } else {
                self.generate_fallback_name(&type_def)
            };
            self.type_names.insert(type_id, name);
        }
    }

    fn collect_naming_contexts(
        &self,
        type_id: TypeId,
        ctx: &NamingContext,
        contexts: &mut HashMap<TypeId, NamingContext>,
    ) {
        if contexts.contains_key(&type_id) {
            return;
        }

        let Some(type_def) = self.types.get(type_id) else {
            return;
        };

        match type_def.classify() {
            TypeData::Primitive(_) => {}
            TypeData::Wrapper {
                kind: TypeKind::Alias,
                ..
            } => {}
            TypeData::Wrapper { inner, .. } => {
                self.collect_naming_contexts(inner, ctx, contexts);
            }
            TypeData::Composite {
                kind: TypeKind::Struct,
                ..
            } => {
                contexts.entry(type_id).or_insert_with(|| ctx.clone());
                for member in self.types.members_of(&type_def) {
                    let field_name = self.strings.get(member.name);
                    let (inner_type, _) = self.types.unwrap_optional(member.type_id);
                    let field_ctx = NamingContext {
                        def_name: ctx.def_name.clone(),
                        field_name: Some(field_name.to_string()),
                    };
                    self.collect_naming_contexts(inner_type, &field_ctx, contexts);
                }
            }
            TypeData::Composite {
                kind: TypeKind::Enum,
                ..
            } => {
                contexts.entry(type_id).or_insert_with(|| ctx.clone());
            }
            TypeData::Composite { .. } => {}
        }
    }

    pub(super) fn needs_generated_name(&self, type_def: &plotnik_bytecode::TypeDef) -> bool {
        matches!(
            type_def.classify(),
            TypeData::Composite {
                kind: TypeKind::Struct | TypeKind::Enum,
                ..
            }
        )
    }

    pub(super) fn generate_contextual_name(&mut self, ctx: &NamingContext) -> String {
        let base = if let Some(field) = &ctx.field_name {
            format!("{}{}", to_pascal_case(&ctx.def_name), to_pascal_case(field))
        } else {
            to_pascal_case(&ctx.def_name)
        };
        self.unique_name(&base)
    }

    pub(super) fn generate_fallback_name(
        &mut self,
        type_def: &plotnik_bytecode::TypeDef,
    ) -> String {
        let base = match type_def.classify() {
            TypeData::Composite {
                kind: TypeKind::Struct,
                ..
            } => "Struct",
            TypeData::Composite {
                kind: TypeKind::Enum,
                ..
            } => "Enum",
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
