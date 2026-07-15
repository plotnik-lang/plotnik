//! Rust-specific representation decisions shared by declarations and decoding.
//!
//! This model assigns Rust identifiers and computes lifetime/boxing facts once.
//! Renderers borrow it; they never collect or rename the result schema again.

use std::collections::HashMap;

use crate::compiler::analyze::result::{ResultItem, ResultSchema};
use crate::compiler::analyze::types::type_shape::TypeId;
use crate::compiler::emit::targets::rust::ident::rust_scope_idents;
use crate::core::Symbol;

use super::representation::{LifetimeUsage, TypeFacts};

pub(crate) struct TypeModel<'a> {
    schema: ResultSchema<'a>,
    facts: TypeFacts,
    /// Hygienic module-scope identifier for every declared item name.
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

impl<'a> TypeModel<'a> {
    pub(crate) fn new(schema: ResultSchema<'a>) -> Self {
        let facts = TypeFacts::compute(schema.types);
        let interner = schema.interner;
        let items = schema.entry_point_items();
        let idents = rust_scope_idents(items.iter().map(|item| interner.resolve(item.name)));
        let item_idents = items
            .iter()
            .zip(idents)
            .map(|(item, ident)| (item.name, ident))
            .collect();
        Self {
            schema,
            facts,
            item_idents,
        }
    }

    pub(crate) fn schema(&self) -> &ResultSchema<'a> {
        &self.schema
    }

    pub(crate) fn items(&self) -> &[ResultItem] {
        self.schema.entry_point_items()
    }

    pub(crate) fn item_ident(&self, name: Symbol) -> &str {
        self.item_idents
            .get(&name)
            .expect("every declared item name has an identifier")
    }

    pub(super) fn lifetime_usage(&self, ty: TypeId) -> LifetimeUsage {
        self.facts.lifetime_usage(ty)
    }

    /// Whether a `Ref` occurrence rendered at `context` uses `Box<...>`.
    pub(crate) fn is_boxed_ref(&self, context: TypeContext, ref_ty: TypeId) -> bool {
        context
            .cut
            .is_some_and(|item| self.facts.is_boxed_in(item, ref_ty))
    }
}
