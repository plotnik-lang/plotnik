use std::collections::HashMap;

use crate::compiler::analyze::result::ResultSchema;
use crate::compiler::analyze::types::type_shape::TypeId;
use crate::compiler::emit::targets::rust::ident::rust_scope_idents;
use crate::core::Symbol;

use super::representation::TypeFacts;

pub(crate) struct TypeModel {
    facts: TypeFacts,
    item_idents: HashMap<Symbol, String>,
}

#[derive(Clone, Copy)]
pub(crate) struct TypeContext {
    cut: Option<TypeId>,
}

impl TypeContext {
    pub(crate) fn item(item_ty: TypeId) -> Self {
        Self { cut: Some(item_ty) }
    }

    pub(crate) fn list_element(self) -> Self {
        Self { cut: None }
    }
}

impl TypeModel {
    pub(crate) fn new(schema: ResultSchema<'_>) -> Self {
        let facts = TypeFacts::compute(schema.types);
        let interner = schema.interner;
        let items = schema.entrypoint_items();
        let idents = rust_scope_idents(items.iter().map(|item| interner.resolve(item.name)));
        let item_idents = items
            .iter()
            .zip(idents)
            .map(|(item, ident)| (item.name, ident))
            .collect();
        Self { facts, item_idents }
    }

    pub(crate) fn item_ident(&self, name: Symbol) -> &str {
        self.item_idents
            .get(&name)
            .expect("every declared item name has an identifier")
    }

    pub(crate) fn is_boxed_ref(&self, context: TypeContext, ref_ty: TypeId) -> bool {
        context
            .cut
            .is_some_and(|item| self.facts.is_boxed_in(item, ref_ty))
    }
}
