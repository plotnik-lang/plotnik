//! Target-neutral output schema and capture-member layout.
//!
//! Analysis assigns types and names; this view turns them into the one ordered
//! output model every downstream consumer shares. In particular, absolute
//! `Set`/`EnumOpen` member slots are assigned here, before bytecode or any
//! source backend chooses a representation.

use std::collections::{BTreeMap, HashMap, HashSet};

use thiserror::Error;

use crate::compiler::analyze::AnalysisArtifacts;
use crate::compiler::analyze::refs::DependencyAnalysis;
use crate::compiler::analyze::types::TypeAnalysis;
use crate::compiler::analyze::types::type_shape::{FieldInfo, TYPE_VOID, TypeId, TypeShape};
use crate::core::{Interner, Symbol};

const MAX_MEMBERS: usize = u16::MAX as usize;
const MAX_SCOPE_MEMBERS: usize = u8::MAX as usize;

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub(crate) enum OutputSchemaError {
    #[error("too many type members: {0} (max {MAX_MEMBERS})")]
    Members(usize),
    #[error("too many struct fields: {0} (max {MAX_SCOPE_MEMBERS})")]
    Fields(usize),
    #[error("too many enum variants: {0} (max {MAX_SCOPE_MEMBERS})")]
    Variants(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OutputItemKind {
    Struct,
    Enum,
    Alias,
    /// A callable void definition has a nominal marker and a `matches` API,
    /// but no replay value.
    VoidDef,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OutputItem {
    pub(crate) name: Symbol,
    pub(crate) ty: TypeId,
    pub(crate) kind: OutputItemKind,
}

impl OutputItem {
    fn for_output(name: Symbol, ty: TypeId, shape: &TypeShape) -> Self {
        let kind = match shape {
            TypeShape::Struct(_) => OutputItemKind::Struct,
            TypeShape::Enum(_) => OutputItemKind::Enum,
            _ => OutputItemKind::Alias,
        };
        Self { name, ty, kind }
    }

    fn void_definition(name: Symbol) -> Self {
        Self {
            name,
            ty: TYPE_VOID,
            kind: OutputItemKind::VoidDef,
        }
    }

    pub(crate) fn is_composite(self) -> bool {
        matches!(self.kind, OutputItemKind::Struct | OutputItemKind::Enum)
    }

    pub(crate) fn is_struct(self) -> bool {
        self.kind == OutputItemKind::Struct
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CaptureScopeKind {
    Struct,
    Enum,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CaptureMemberKind {
    Field(FieldInfo),
    Variant(TypeId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CaptureMember {
    pub(crate) name: Symbol,
    pub(crate) kind: CaptureMemberKind,
}

#[derive(Clone, Debug)]
pub(crate) struct CaptureScope {
    kind: CaptureScopeKind,
    base: u16,
    members: Vec<CaptureMember>,
}

impl CaptureScope {
    pub(crate) fn kind(&self) -> CaptureScopeKind {
        self.kind
    }

    pub(crate) fn base(&self) -> u16 {
        self.base
    }

    pub(crate) fn members(&self) -> &[CaptureMember] {
        &self.members
    }

    pub(crate) fn absolute_index(&self, relative: u16) -> u16 {
        assert!(
            usize::from(relative) < self.members.len(),
            "capture member reference was validated against its parent scope"
        );
        self.base
            .checked_add(relative)
            .expect("capture layout validates the u16 member space")
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CaptureLayout {
    scopes: BTreeMap<TypeId, CaptureScope>,
    member_count: usize,
}

impl CaptureLayout {
    pub(super) fn build(
        types: &TypeAnalysis,
        ordered_types: &[TypeId],
    ) -> Result<Self, OutputSchemaError> {
        let mut scopes = BTreeMap::new();
        let mut member_count = 0;
        for &type_id in ordered_types {
            let (kind, members): (CaptureScopeKind, Vec<CaptureMember>) =
                match types.expect_type_shape(type_id) {
                    TypeShape::Struct(fields) => (
                        CaptureScopeKind::Struct,
                        fields
                            .iter()
                            .map(|(&name, &info)| CaptureMember {
                                name,
                                kind: CaptureMemberKind::Field(info),
                            })
                            .collect(),
                    ),
                    TypeShape::Enum(variants) => (
                        CaptureScopeKind::Enum,
                        variants
                            .iter()
                            .map(|(&name, &payload)| CaptureMember {
                                name,
                                kind: CaptureMemberKind::Variant(payload),
                            })
                            .collect(),
                    ),
                    _ => continue,
                };

            let base = u16::try_from(member_count)
                .map_err(|_| OutputSchemaError::Members(member_count))?;
            for _ in &members {
                if member_count >= MAX_MEMBERS {
                    return Err(OutputSchemaError::Members(member_count + 1));
                }
                member_count += 1;
            }
            match kind {
                CaptureScopeKind::Struct if members.len() > MAX_SCOPE_MEMBERS => {
                    return Err(OutputSchemaError::Fields(members.len()));
                }
                CaptureScopeKind::Enum if members.len() > MAX_SCOPE_MEMBERS => {
                    return Err(OutputSchemaError::Variants(members.len()));
                }
                _ => {}
            }
            scopes.insert(
                type_id,
                CaptureScope {
                    kind,
                    base,
                    members,
                },
            );
        }

        Ok(Self {
            scopes,
            member_count,
        })
    }

    pub(crate) fn scope(&self, type_id: TypeId) -> Option<&CaptureScope> {
        self.scopes.get(&type_id)
    }

    pub(crate) fn member_base(&self, type_id: TypeId) -> Option<u16> {
        self.scope(type_id).map(CaptureScope::base)
    }

    pub(crate) fn member_count(&self) -> usize {
        self.member_count
    }
}

/// The name-assigned, reachable output model shared by bytecode and generated
/// source backends. It contains semantic shapes and capture slots only; target
/// representation choices such as Rust `Box` placement live downstream.
#[derive(Clone)]
pub(crate) struct OutputSchema<'a> {
    pub(crate) types: &'a TypeAnalysis,
    pub(crate) deps: &'a DependencyAnalysis,
    pub(crate) interner: &'a Interner,
    ordered_types: Vec<TypeId>,
    type_names: HashMap<TypeId, Symbol>,
    items: Vec<OutputItem>,
    layout: CaptureLayout,
}

impl<'a> OutputSchema<'a> {
    pub(crate) fn from_artifacts(
        artifacts: AnalysisArtifacts<'a>,
    ) -> Result<Self, OutputSchemaError> {
        Self::new(
            artifacts.type_analysis,
            artifacts.dependency_analysis,
            artifacts.interner,
        )
    }

    pub(crate) fn new(
        types: &'a TypeAnalysis,
        deps: &'a DependencyAnalysis,
        interner: &'a Interner,
    ) -> Result<Self, OutputSchemaError> {
        let ordered_types = collect_ordered_types(types);
        let layout = CaptureLayout::build(types, &ordered_types)?;
        let type_names: HashMap<TypeId, Symbol> = types.iter_type_names().collect();
        let items = collect_items(types, deps, &type_names);
        Ok(Self {
            types,
            deps,
            interner,
            ordered_types,
            type_names,
            items,
            layout,
        })
    }

    pub(crate) fn ordered_types(&self) -> &[TypeId] {
        &self.ordered_types
    }

    pub(crate) fn type_name_of(&self, type_id: TypeId) -> Option<Symbol> {
        self.type_names.get(&type_id).copied()
    }

    pub(crate) fn items(&self) -> &[OutputItem] {
        &self.items
    }

    pub(crate) fn layout(&self) -> &CaptureLayout {
        &self.layout
    }
}

fn collect_items(
    types: &TypeAnalysis,
    deps: &DependencyAnalysis,
    type_names: &HashMap<TypeId, Symbol>,
) -> Vec<OutputItem> {
    let mut collector = ItemCollector {
        types,
        type_names,
        declared: HashSet::new(),
        items: Vec::new(),
    };
    for (def_id, output) in types.iter_def_output() {
        let name = deps.def_name_sym(def_id);
        if output == TYPE_VOID {
            if types.is_entrypoint_def(def_id) {
                collector.items.push(OutputItem::void_definition(name));
            }
            continue;
        }
        collector.add_item(name, output);
    }
    collector.items
}

struct ItemCollector<'a> {
    types: &'a TypeAnalysis,
    type_names: &'a HashMap<TypeId, Symbol>,
    declared: HashSet<Symbol>,
    items: Vec<OutputItem>,
}

impl ItemCollector<'_> {
    fn add_item(&mut self, name: Symbol, ty: TypeId) {
        if !self.declared.insert(name) {
            return;
        }

        let item = OutputItem::for_output(name, ty, self.types.expect_type_shape(ty));
        self.items.push(item);
        match item.kind {
            OutputItemKind::Struct | OutputItemKind::Enum => self.collect_composite_children(ty),
            OutputItemKind::Alias => self.collect_alias_interior(ty),
            OutputItemKind::VoidDef => unreachable!("void definitions are inserted directly"),
        }
    }

    fn collect_composite_children(&mut self, ty: TypeId) {
        match self.types.expect_type_shape(ty) {
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
                    let TypeShape::Struct(fields) = self.types.expect_type_shape(payload) else {
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

    fn collect_position(&mut self, ty: TypeId) {
        match self.types.expect_type_shape(ty) {
            TypeShape::Struct(_) | TypeShape::Enum(_) => {
                let name = *self
                    .type_names
                    .get(&ty)
                    .expect("naming pass names every non-payload composite");
                self.add_item(name, ty);
            }
            TypeShape::Custom(_) => {
                if let Some(&name) = self.type_names.get(&ty) {
                    self.add_item(name, ty);
                }
            }
            TypeShape::Array { element, .. } => self.collect_position(*element),
            TypeShape::Optional(inner) => self.collect_position(*inner),
            TypeShape::Node | TypeShape::Ref(_) => {}
            TypeShape::Void => unreachable!("void cannot appear in an output position"),
        }
    }

    fn collect_alias_interior(&mut self, ty: TypeId) {
        match self.types.expect_type_shape(ty) {
            TypeShape::Array { element, .. } => self.collect_position(*element),
            TypeShape::Optional(inner) => self.collect_position(*inner),
            TypeShape::Node | TypeShape::Custom(_) | TypeShape::Ref(_) => {}
            TypeShape::Struct(_) | TypeShape::Enum(_) | TypeShape::Void => {
                unreachable!("alias items cover non-composite outputs only")
            }
        }
    }
}

/// Custom types in the exact post-order the wire table historically used.
pub(super) fn collect_ordered_types(types: &TypeAnalysis) -> Vec<TypeId> {
    let mut collector = TypeCollector::new();
    for (_def_id, type_id) in types.iter_def_output() {
        collector.collect(type_id, types);
        if !matches!(types.expect_type_shape(type_id), TypeShape::Ref(_)) {
            continue;
        }
        if collector.seen.insert(type_id) {
            collector.out.push(type_id);
        }
    }
    collector.out
}

struct TypeCollector {
    out: Vec<TypeId>,
    seen: HashSet<TypeId>,
}

impl TypeCollector {
    fn new() -> Self {
        Self {
            out: Vec::new(),
            seen: HashSet::new(),
        }
    }

    fn collect(&mut self, type_id: TypeId, types: &TypeAnalysis) {
        if type_id.is_builtin() || self.seen.contains(&type_id) {
            return;
        }
        let shape = types.expect_type_shape(type_id);
        if let TypeShape::Ref(def_id) = shape {
            self.collect(types.expect_def_output(*def_id), types);
            return;
        }

        self.seen.insert(type_id);
        for child in shape.child_type_ids() {
            self.collect(child, types);
        }
        if matches!(
            shape,
            TypeShape::Struct(_)
                | TypeShape::Enum(_)
                | TypeShape::Array { .. }
                | TypeShape::Optional(_)
                | TypeShape::Custom(_)
        ) {
            self.out.push(type_id);
        }
    }
}
