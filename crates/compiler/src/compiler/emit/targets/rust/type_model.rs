use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use crate::compiler::analyze::result::{ResultItem, ResultSchema};
use crate::compiler::analyze::types::type_shape::TypeId;
use crate::compiler::diagnostics::SourceId;
use crate::compiler::emit::targets::rust::ident::rust_scope_idents;
use crate::core::Symbol;

use super::representation::{LifetimeUsage, TypeFacts};

#[derive(Clone)]
pub(crate) struct TypeModel<'a> {
    schema: ResultSchema<'a>,
    facts: Rc<TypeFacts>,
    item_idents: HashMap<Symbol, String>,
    current_source: Option<SourceId>,
    item_paths: HashMap<Symbol, String>,
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
        let facts = Rc::new(TypeFacts::compute(schema.types));
        let interner = schema.interner;
        let items = schema.entrypoint_items();
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
            current_source: None,
            item_paths: HashMap::new(),
        }
    }

    pub(crate) fn in_module(
        mut self,
        source: SourceId,
        modules: &BTreeMap<SourceId, String>,
    ) -> Self {
        self.current_source = Some(source);
        self.item_paths = self
            .item_idents
            .iter()
            .map(|(&name, ident)| {
                let owner = self.item_source(name);
                let path = if owner == source {
                    ident.clone()
                } else {
                    let module = modules
                        .get(&owner)
                        .expect("every foreign public item owner has a Rust module");
                    format!("super::{module}::{ident}")
                };
                (name, path)
            })
            .collect();
        self
    }

    pub(crate) fn source(&self) -> Option<SourceId> {
        self.current_source
    }

    pub(crate) fn item_source(&self, name: Symbol) -> SourceId {
        self.schema
            .types
            .type_source(name)
            .expect("module placement requires unambiguous analyzed ownership")
    }

    pub(crate) fn item_path(&self, name: Symbol) -> &str {
        if self.current_source.is_none() {
            return self.item_ident(name);
        }
        self.item_paths
            .get(&name)
            .expect("module context provides a path for every public item")
    }

    pub(crate) fn schema(&self) -> &ResultSchema<'a> {
        &self.schema
    }

    pub(crate) fn items(&self) -> &[ResultItem] {
        self.schema.entrypoint_items()
    }

    pub(crate) fn item_ident(&self, name: Symbol) -> &str {
        self.item_idents
            .get(&name)
            .expect("every declared item name has an identifier")
    }

    pub(super) fn lifetime_usage(&self, ty: TypeId) -> LifetimeUsage {
        self.facts.lifetime_usage(ty)
    }

    pub(crate) fn is_boxed_ref(&self, context: TypeContext, ref_ty: TypeId) -> bool {
        context
            .cut
            .is_some_and(|item| self.facts.is_boxed_in(item, ref_ty))
    }
}
