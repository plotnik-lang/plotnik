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

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use crate::compiler::analyze::refs::DependencyAnalysis;
use crate::compiler::analyze::types::TypeAnalysis;
use crate::compiler::analyze::types::type_shape::{FieldInfo, TYPE_VOID, TypeId, TypeShape};
use crate::compiler::ids::DefId;
use crate::core::{Interner, Symbol};

use super::Config;
use super::analysis::TypeFacts;
use super::idents::scope_idents;

const DERIVES: &str = "#[derive(Debug, Clone, PartialEq, Eq, Hash)]";

pub(crate) struct Emitter<'a> {
    pub(super) types: &'a TypeAnalysis,
    pub(super) deps: &'a DependencyAnalysis,
    pub(super) interner: &'a Interner,
    pub(super) config: &'a Config,
    pub(super) facts: TypeFacts,
    /// Names assigned by the naming pass. Consulted only for nominal shapes
    /// (struct/enum, fresh ids) and `Custom` (interned per symbol) — entries
    /// for shared builtin/wrapper ids are definition-name noise.
    type_names: HashMap<TypeId, Symbol>,
    declared: HashSet<Symbol>,
    items: Vec<Item>,
    /// Hygienic module-scope identifier for every declared item name.
    item_idents: HashMap<Symbol, String>,
    uses_node: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct Item {
    pub(crate) name: Symbol,
    pub(crate) ty: TypeId,
    pub(crate) kind: ItemKind,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum ItemKind {
    Struct,
    Enum,
    Alias,
    /// A void definition: matches without producing data, so no type. Renders
    /// as a marker comment so the definition doesn't silently vanish.
    VoidDef,
}

impl Item {
    fn output(name: Symbol, ty: TypeId, shape: &TypeShape) -> Self {
        Self {
            name,
            ty,
            kind: ItemKind::from_output_shape(shape),
        }
    }

    fn void_definition(name: Symbol) -> Self {
        Self {
            name,
            ty: TYPE_VOID,
            kind: ItemKind::VoidDef,
        }
    }

    pub(crate) fn is_composite(&self) -> bool {
        matches!(self.kind, ItemKind::Struct | ItemKind::Enum)
    }

    pub(crate) fn is_struct(&self) -> bool {
        self.kind == ItemKind::Struct
    }

    pub(crate) fn has_reader(&self) -> bool {
        self.kind != ItemKind::VoidDef
    }
}

impl ItemKind {
    fn from_output_shape(shape: &TypeShape) -> Self {
        match shape {
            TypeShape::Struct(_) => ItemKind::Struct,
            TypeShape::Enum(_) => ItemKind::Enum,
            _ => ItemKind::Alias,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct TypeContext {
    cut: Option<TypeId>,
}

impl TypeContext {
    pub(super) fn item(item_ty: TypeId) -> Self {
        Self { cut: Some(item_ty) }
    }

    fn array_element(self) -> Self {
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
        Self {
            types,
            deps,
            interner,
            config,
            facts: TypeFacts::compute(types),
            type_names: types.iter_type_names().collect(),
            declared: HashSet::new(),
            items: Vec::new(),
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
        emitter.collect();
        emitter.assign_item_idents();
        emitter
    }

    /// The declared items, in emission order (definitions first, named
    /// composites parent-first after their owner).
    pub(crate) fn items(&self) -> &[Item] {
        &self.items
    }

    /// Whether the type's rendering mentions `'t` (transitively holds a node).
    pub(crate) fn needs_lifetime(&self, ty: TypeId) -> bool {
        self.facts.needs_lifetime(ty)
    }

    /// Whether a `Ref` occurrence rendered by value inside `item_ty`'s
    /// declaration renders as `Box<...>`. `None` is the under-an-array
    /// context: `Vec` already indirects, so nothing below it boxes. Sibling
    /// backends (the trace readers) must ask with the same context the type
    /// renderer used, or their `Box::new` placement drifts from the types.
    pub(crate) fn is_boxed_ref(&self, item_ty: Option<TypeId>, ref_ty: TypeId) -> bool {
        item_ty.is_some_and(|item| self.facts.is_boxed_in(item, ref_ty))
    }

    /// The naming-pass name of a nominal type, when it has one. Trustworthy
    /// only for the shapes `type_names` covers (see the field's doc).
    pub(crate) fn type_name_of(&self, ty: TypeId) -> Option<Symbol> {
        self.type_names.get(&ty).copied()
    }

    pub(super) fn emit(mut self) -> String {
        self.collect();
        self.assign_item_idents();

        let mut sections: Vec<String> = Vec::new();
        for item in self.items.clone() {
            sections.push(self.render_item(&item));
        }
        if self.config.serde {
            for item in self.items.clone() {
                if item.is_composite() {
                    sections.push(self.serde_impl(&item));
                }
            }
        }

        let mut out = String::new();
        if self.uses_node {
            let rt = &self.config.rt_crate;
            out.push_str(&format!("use {rt}::Node;\n\n"));
        }
        out.push_str(&sections.join("\n\n"));
        out.push('\n');
        out
    }

    fn collect(&mut self) {
        let defs: Vec<(DefId, TypeId)> = self.types.iter_def_output().collect();
        for (def_id, output) in defs {
            let name = self.deps.def_name_sym(def_id);
            if output == TYPE_VOID {
                self.items.push(Item::void_definition(name));
                continue;
            }
            self.add_item(name, output);
        }
    }

    fn add_item(&mut self, name: Symbol, ty: TypeId) {
        // The same name recurs only for structurally identical types (nominal
        // twins from repeated annotations); one declaration serves them all.
        if !self.declared.insert(name) {
            return;
        }

        let item = Item::output(name, ty, self.types.expect_type_shape(ty));
        let kind = item.kind;
        self.items.push(item);

        match kind {
            ItemKind::Struct | ItemKind::Enum => self.collect_composite_children(ty),
            ItemKind::Alias => self.collect_alias_interior(ty),
            ItemKind::VoidDef => unreachable!("void definitions never become named items"),
        }
    }

    fn collect_composite_children(&mut self, ty: TypeId) {
        let types = self.types;
        match types.expect_type_shape(ty) {
            TypeShape::Struct(fields) => {
                for info in fields.values() {
                    self.collect_position(info.type_id);
                }
            }
            TypeShape::Enum(variants) => {
                for &payload in variants.values() {
                    if payload == TYPE_VOID {
                        continue;
                    }
                    let TypeShape::Struct(fields) = types.expect_type_shape(payload) else {
                        unreachable!("enum variant payload is void or an anonymous struct");
                    };
                    for info in fields.values() {
                        self.collect_position(info.type_id);
                    }
                }
            }
            _ => unreachable!("children collection runs on composites only"),
        }
    }

    /// Discover the named item (if any) behind a use-site position.
    fn collect_position(&mut self, ty: TypeId) {
        let types = self.types;
        match types.expect_type_shape(ty) {
            TypeShape::Struct(_) | TypeShape::Enum(_) => {
                let name = *self
                    .type_names
                    .get(&ty)
                    .expect("naming pass names every composite outside enum-variant payloads");
                self.add_item(name, ty);
            }
            // A named `Custom` is a `:: TypeName` alias of Node; a `:: Node`
            // restatement stays unnamed and renders as plain `Node`.
            TypeShape::Custom(_) => {
                if let Some(&name) = self.type_names.get(&ty) {
                    self.add_item(name, ty);
                }
            }
            TypeShape::Array { element, .. } => self.collect_position(*element),
            TypeShape::Optional(inner) => self.collect_position(*inner),
            // A ref's target declares its own item via its definition.
            TypeShape::Node | TypeShape::Ref(_) => {}
            TypeShape::Void => unreachable!("void cannot appear in an output position"),
        }
    }

    /// An alias item's body can still reach named composites through wrappers
    /// (`Items = (Entry)*` aliases `Vec<Entry>`).
    fn collect_alias_interior(&mut self, ty: TypeId) {
        let types = self.types;
        match types.expect_type_shape(ty) {
            TypeShape::Array { element, .. } => self.collect_position(*element),
            TypeShape::Optional(inner) => self.collect_position(*inner),
            TypeShape::Node | TypeShape::Custom(_) | TypeShape::Ref(_) => {}
            TypeShape::Struct(_) | TypeShape::Enum(_) | TypeShape::Void => {
                unreachable!("alias items cover non-composite outputs only")
            }
        }
    }

    fn assign_item_idents(&mut self) {
        let interner = self.interner;
        let idents = scope_idents(self.items.iter().map(|item| interner.resolve(item.name)));
        for (item, ident) in self.items.iter().zip(idents) {
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
            ItemKind::VoidDef => format!(
                "// `{}` matches without producing data; no output type.",
                self.interner.resolve(item.name)
            ),
        }
    }

    fn render_struct(&mut self, item: &Item) -> String {
        let types = self.types;
        let interner = self.interner;
        let TypeShape::Struct(fields) = types.expect_type_shape(item.ty) else {
            unreachable!("struct item must have a struct shape");
        };
        let field_idents = scope_idents(fields.keys().map(|&sym| interner.resolve(sym)));
        let ident = self.item_ident(item.name).to_string();
        let lt = self.lifetime_args(item.ty);

        let mut out = format!("{DERIVES}\npub struct {ident}{lt} {{\n");
        for (info, field_ident) in fields.values().zip(&field_idents) {
            let field_ty = self.field_type(TypeContext::item(item.ty), info);
            writeln!(out, "    pub {field_ident}: {field_ty},")
                .expect("writing to a String is infallible");
        }
        out.push('}');
        out
    }

    fn render_enum(&mut self, item: &Item) -> String {
        let types = self.types;
        let interner = self.interner;
        let TypeShape::Enum(variants) = types.expect_type_shape(item.ty) else {
            unreachable!("enum item must have an enum shape");
        };
        let variant_idents = scope_idents(variants.keys().map(|&sym| interner.resolve(sym)));
        let ident = self.item_ident(item.name).to_string();
        let lt = self.lifetime_args(item.ty);

        let mut out = format!("{DERIVES}\npub enum {ident}{lt} {{\n");
        for ((_, &payload), variant_ident) in variants.iter().zip(&variant_idents) {
            if payload == TYPE_VOID {
                writeln!(out, "    {variant_ident},").expect("writing to a String is infallible");
                continue;
            }
            let TypeShape::Struct(fields) = types.expect_type_shape(payload) else {
                unreachable!("enum variant payload is void or an anonymous struct");
            };
            let field_idents = scope_idents(fields.keys().map(|&sym| interner.resolve(sym)));
            let rendered: Vec<String> = fields
                .values()
                .zip(&field_idents)
                .map(|(info, field_ident)| {
                    format!(
                        "{field_ident}: {}",
                        self.field_type(TypeContext::item(item.ty), info)
                    )
                })
                .collect();
            writeln!(out, "    {variant_ident} {{ {} }},", rendered.join(", "))
                .expect("writing to a String is infallible");
        }
        out.push('}');
        out
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
        let types = self.types;
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
        let types = self.types;
        match types.expect_type_shape(ty) {
            TypeShape::Node => self.node_type(),
            TypeShape::Custom(_) => match self.type_names.get(&ty) {
                Some(&name) => self.named_type(name, ty),
                None => self.node_type(),
            },
            TypeShape::Struct(_) | TypeShape::Enum(_) => {
                let name = *self
                    .type_names
                    .get(&ty)
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
        let target = self.types.expect_def_output(def_id);
        if target == TYPE_VOID {
            return self.node_type();
        }

        let name = self.deps.def_name_sym(def_id);
        let base = self.named_type(name, target);
        if self.is_boxed_ref(context.cut, ref_ty) {
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
