//! Item collection and rendering.
//!
//! One item per named type: every non-void definition (struct, enum, or a
//! `pub type` alias for scalar/wrapper outputs), plus every named composite
//! its output reaches, emitted parent-first right after their owner. Names
//! come verbatim from the naming pass; the only unnamed composites are enum
//! variant payload structs, which render inline as struct variants.
//!
//! `Option`/`Vec`/`Box` are spelled absolutely (`::core::option::Option`)
//! because a definition may legitimately be named `Option` and item
//! declarations shadow the prelude inside the generated module. `Node` alone
//! can stay bare: the naming pass reserves it.

use std::collections::HashMap;

use crate::compiler::analyze::output::OutputSchema;
pub(crate) use crate::compiler::analyze::output::{OutputItem as Item, OutputItemKind as ItemKind};
use crate::compiler::analyze::refs::DependencyAnalysis;
use crate::compiler::analyze::types::TypeAnalysis;
use crate::compiler::analyze::types::type_shape::{FieldInfo, TYPE_VOID, TypeId, TypeShape};
use crate::compiler::codegen::emit::names::rust_scope_idents;
use crate::compiler::codegen::emit::sink::Sink;
use crate::compiler::ids::DefId;
use crate::core::{Interner, Symbol};

use super::Config;
use super::analysis::TypeFacts;

const DERIVES: &str = "#[derive(Debug, Clone, PartialEq, Eq, Hash)]";

pub(crate) struct Emitter<'a> {
    pub(super) schema: OutputSchema<'a>,
    pub(super) config: &'a Config,
    pub(super) facts: TypeFacts,
    /// Hygienic module-scope identifier for every declared item name.
    item_idents: HashMap<Symbol, String>,
    uses_node: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct TypeContext {
    cut: Option<TypeId>,
}

impl TypeContext {
    pub(crate) fn item(item_ty: TypeId) -> Self {
        Self { cut: Some(item_ty) }
    }

    pub(crate) fn array_element(self) -> Self {
        Self { cut: None }
    }
}

impl<'a> Emitter<'a> {
    pub(super) fn new(
        types: &'a TypeAnalysis,
        deps: &'a DependencyAnalysis,
        interner: &'a Interner,
        config: &'a Config,
    ) -> Self {
        let schema = OutputSchema::new(types, deps, interner)
            .expect("bytecode dry-run validated the output schema");
        Self {
            schema,
            config,
            facts: TypeFacts::compute(types),
            item_idents: HashMap::new(),
            uses_node: false,
        }
    }

    /// A collected, name-assigned view for sibling backends: the matcher's
    /// typed-replay readers must spell items and fields exactly as the type
    /// declarations do, so they consult this model instead of re-deriving
    /// names. Never call [`Self::emit`] on a model — it collects again.
    pub(crate) fn model(
        types: &'a TypeAnalysis,
        deps: &'a DependencyAnalysis,
        interner: &'a Interner,
        config: &'a Config,
    ) -> Self {
        let mut emitter = Self::new(types, deps, interner, config);
        emitter.assign_item_idents();
        emitter
    }

    /// The declared items, in emission order (definitions first, named
    /// composites parent-first after their owner).
    pub(crate) fn items(&self) -> &[Item] {
        self.schema.items()
    }

    /// Whether the type's rendering mentions `'t` (transitively holds a node).
    pub(crate) fn needs_lifetime(&self, ty: TypeId) -> bool {
        self.facts.needs_lifetime(ty)
    }

    /// Whether a `Ref` occurrence rendered at `context` uses `Box<...>`.
    /// Sibling backends (the trace readers) must ask with the same context the
    /// type renderer used, or their `Box::new` placement drifts from the types.
    pub(crate) fn is_boxed_ref(&self, context: TypeContext, ref_ty: TypeId) -> bool {
        context
            .cut
            .is_some_and(|item| self.facts.is_boxed_in(item, ref_ty))
    }

    /// The naming-pass name of a nominal type, when it has one. Trustworthy
    /// only for the shapes `type_names` covers (see the field's doc).
    pub(crate) fn type_name_of(&self, ty: TypeId) -> Option<Symbol> {
        self.schema.type_name_of(ty)
    }

    pub(super) fn emit(mut self) -> String {
        self.assign_item_idents();

        let sections = self.render_sections();
        let mut out = Sink::<()>::new();
        if self.uses_node {
            let rt = &self.config.rt_crate;
            out.push(&format!("use {rt}::Node;\n\n"));
        }
        out.push(&sections.join("\n\n"));
        out.push("\n");
        out.plain().to_string()
    }

    fn render_sections(&mut self) -> Vec<String> {
        let mut sections: Vec<String> = Vec::new();
        for item in self.schema.items().to_vec() {
            sections.push(self.render_item(&item));
        }

        if !self.config.serde {
            return sections;
        }

        for item in self.schema.items().to_vec() {
            if !item.is_composite() {
                continue;
            }
            sections.push(self.serde_impl(&item));
        }
        sections
    }

    fn assign_item_idents(&mut self) {
        let interner = self.schema.interner;
        let items = self.schema.items();
        let idents = rust_scope_idents(items.iter().map(|item| interner.resolve(item.name)));
        for (item, ident) in items.iter().zip(idents) {
            self.item_idents.insert(item.name, ident);
        }
    }

    pub(crate) fn item_ident(&self, name: Symbol) -> &str {
        self.item_idents
            .get(&name)
            .expect("every declared item name has an identifier")
    }

    fn render_item(&mut self, item: &Item) -> String {
        match item.kind {
            ItemKind::Struct => self.render_struct(item),
            ItemKind::Enum => self.render_enum(item),
            ItemKind::Alias => self.render_alias(item),
            ItemKind::VoidDef => self.render_void_marker(item),
        }
    }

    fn render_void_marker(&mut self, item: &Item) -> String {
        let ident = self.item_ident(item.name).to_string();
        format!("{DERIVES}\npub struct {ident};")
    }

    fn render_struct(&mut self, item: &Item) -> String {
        let types = self.schema.types;
        let interner = self.schema.interner;
        let TypeShape::Struct(fields) = types.expect_type_shape(item.ty) else {
            unreachable!("struct item must have a struct shape");
        };
        let field_idents = rust_scope_idents(fields.keys().map(|&sym| interner.resolve(sym)));
        let ident = self.item_ident(item.name).to_string();
        let lt = self.lifetime_args(item.ty);

        let mut out = Sink::<()>::new();
        out.line(DERIVES);
        out.line(&format!("pub struct {ident}{lt} {{"));
        out.indented(|out| {
            for (info, field_ident) in fields.values().zip(&field_idents) {
                let field_ty = self.field_type(TypeContext::item(item.ty), info);
                out.line(&format!("pub {field_ident}: {field_ty},"));
            }
        });
        out.push("}");
        out.plain().to_string()
    }

    fn render_enum(&mut self, item: &Item) -> String {
        let types = self.schema.types;
        let interner = self.schema.interner;
        let TypeShape::Enum(variants) = types.expect_type_shape(item.ty) else {
            unreachable!("enum item must have an enum shape");
        };
        let variant_idents = rust_scope_idents(variants.keys().map(|&sym| interner.resolve(sym)));
        let ident = self.item_ident(item.name).to_string();
        let lt = self.lifetime_args(item.ty);

        let mut out = Sink::<()>::new();
        out.line(DERIVES);
        out.line(&format!("pub enum {ident}{lt} {{"));
        out.indented(|out| {
            for ((_, &payload), variant_ident) in variants.iter().zip(&variant_idents) {
                let payload = self.render_variant_payload(item.ty, payload);
                out.line(&format!("{variant_ident}{payload},"));
            }
        });
        out.push("}");
        out.plain().to_string()
    }

    fn render_variant_payload(&mut self, item_ty: TypeId, payload: TypeId) -> String {
        if payload == TYPE_VOID {
            return String::new();
        }

        let types = self.schema.types;
        let interner = self.schema.interner;
        let TypeShape::Struct(fields) = types.expect_type_shape(payload) else {
            unreachable!("enum variant payload is void or an anonymous struct");
        };
        let field_idents = rust_scope_idents(fields.keys().map(|&sym| interner.resolve(sym)));
        let rendered: Vec<String> = fields
            .values()
            .zip(&field_idents)
            .map(|(info, field_ident)| {
                format!(
                    "{field_ident}: {}",
                    self.field_type(TypeContext::item(item_ty), info)
                )
            })
            .collect();
        format!(" {{ {} }}", rendered.join(", "))
    }

    fn render_alias(&mut self, item: &Item) -> String {
        let ident = self.item_ident(item.name).to_string();
        let lt = self.lifetime_args(item.ty);
        let body = self.alias_body(TypeContext::item(item.ty), item.ty);
        format!("pub type {ident}{lt} = {body};")
    }

    /// An alias item's body: one shape level rendered structurally (the
    /// item's own name must not win), positions below it render as usual.
    ///
    /// Throughout the rendering recursion, `cut` is the item declaration a
    /// by-value path from this position is still inside — the context
    /// [`Self::is_boxed_ref`] keys on. Descending into an array element
    /// clears it: `Vec` indirects, so no cycle below is by-value.
    fn alias_body(&mut self, context: TypeContext, ty: TypeId) -> String {
        let types = self.schema.types;
        match types.expect_type_shape(ty) {
            TypeShape::Node | TypeShape::Custom(_) => self.node_type(),
            TypeShape::Array { element, .. } => {
                format!(
                    "::std::vec::Vec<{}>",
                    self.position_type(context.array_element(), *element)
                )
            }
            TypeShape::Optional(inner) => {
                format!(
                    "::core::option::Option<{}>",
                    self.position_type(context, *inner)
                )
            }
            TypeShape::Ref(def_id) => self.ref_type(context, *def_id, ty),
            TypeShape::Struct(_) | TypeShape::Enum(_) | TypeShape::Void => {
                unreachable!("alias items cover non-composite outputs only")
            }
        }
    }

    /// A field's rendered type: the capture-level `optional` flag wraps one
    /// more `Option` around the base, composing with an already-optional base
    /// exactly like the bytecode type table does (two nulls from two distinct
    /// syntax sites legitimately nest).
    pub(super) fn field_type(&mut self, context: TypeContext, info: &FieldInfo) -> String {
        let base = self.position_type(context, info.type_id);
        if info.optional {
            format!("::core::option::Option<{base}>")
        } else {
            base
        }
    }

    /// Render a type at a use site: named types by name, wrappers inline.
    pub(super) fn position_type(&mut self, context: TypeContext, ty: TypeId) -> String {
        let types = self.schema.types;
        match types.expect_type_shape(ty) {
            TypeShape::Node => self.node_type(),
            TypeShape::Custom(_) => match self.schema.type_name_of(ty) {
                Some(name) => self.named_type(name, ty),
                None => self.node_type(),
            },
            TypeShape::Struct(_) | TypeShape::Enum(_) => {
                let name = self
                    .schema
                    .type_name_of(ty)
                    .expect("naming pass names every composite outside enum-variant payloads");
                self.named_type(name, ty)
            }
            TypeShape::Array { element, .. } => {
                format!(
                    "::std::vec::Vec<{}>",
                    self.position_type(context.array_element(), *element)
                )
            }
            TypeShape::Optional(inner) => {
                format!(
                    "::core::option::Option<{}>",
                    self.position_type(context, *inner)
                )
            }
            TypeShape::Ref(def_id) => self.ref_type(context, *def_id, ty),
            TypeShape::Void => unreachable!("void cannot appear in an output position"),
        }
    }

    fn node_type(&mut self) -> String {
        self.uses_node = true;
        "Node<'t>".to_string()
    }

    fn named_type(&self, name: Symbol, ty: TypeId) -> String {
        let ident = self.item_ident(name);
        let lt = self.lifetime_args(ty);
        format!("{ident}{lt}")
    }

    /// A reference renders as its target definition's type name, boxed when
    /// this occurrence closes a by-value cycle through the enclosing item's
    /// declaration. A void target contributes no value, so the capture holds
    /// the matched node itself.
    fn ref_type(&mut self, context: TypeContext, def_id: DefId, ref_ty: TypeId) -> String {
        let target = self.schema.types.expect_def_output(def_id);
        if target == TYPE_VOID {
            return self.node_type();
        }

        let name = self.schema.deps.def_name_sym(def_id);
        let base = self.named_type(name, target);
        if self.is_boxed_ref(context, ref_ty) {
            format!("::std::boxed::Box<{base}>")
        } else {
            base
        }
    }

    pub(super) fn lifetime_args(&self, ty: TypeId) -> &'static str {
        if self.facts.needs_lifetime(ty) {
            "<'t>"
        } else {
            ""
        }
    }
}
