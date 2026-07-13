//! Target-neutral output schema and capture-member layout.
//!
//! Analysis assigns types and names; this view turns them into the one ordered
//! output model every downstream consumer shares. In particular, absolute
//! composite member slots are assigned here, before bytecode or any
//! source backend chooses a representation.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::OnceLock;

use thiserror::Error;

use crate::compiler::analyze::AnalysisArtifacts;
use crate::compiler::analyze::refs::DependencyAnalysis;
use crate::compiler::analyze::types::TypeAnalysis;
use crate::compiler::analyze::types::type_shape::{
    FieldInfo, TYPE_BOOL, TYPE_NODE, TYPE_TEXT, TYPE_VOID, TypeId, TypeShape,
};
use crate::compiler::ids::DefId;
use crate::core::{Interner, Symbol};

const MAX_MEMBERS: usize = u16::MAX as usize;

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub(crate) enum OutputSchemaError {
    #[error("too many type members: {0} (max {MAX_MEMBERS})")]
    Members(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OutputItemKind {
    Struct,
    Variant,
    Alias,
    /// A selectable match-only definition has a nominal marker and a `matches` API,
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
            TypeShape::Variant(_) => OutputItemKind::Variant,
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
        matches!(self.kind, OutputItemKind::Struct | OutputItemKind::Variant)
    }

    pub(crate) fn is_struct(self) -> bool {
        self.kind == OutputItemKind::Struct
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CaptureScopeKind {
    Struct,
    Variant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CaptureMemberKind {
    Field(FieldInfo),
    Case(TypeId),
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

/// Stable analysis-type to emitted-type numbering shared by every output
/// backend. Built-ins lead in canonical order; custom wire types retain the
/// compiler's historical post-order.
#[derive(Clone, Debug)]
pub(crate) struct OutputTypeLayout {
    builtins: Vec<TypeId>,
    custom_types: Vec<TypeId>,
    output_ids: HashMap<TypeId, u32>,
}

impl OutputTypeLayout {
    fn build(used_builtins: &HashSet<TypeId>, custom_types: Vec<TypeId>) -> Self {
        let builtins = [TYPE_VOID, TYPE_NODE, TYPE_TEXT, TYPE_BOOL]
            .into_iter()
            .filter(|builtin| used_builtins.contains(builtin))
            .collect::<Vec<_>>();

        let output_ids = builtins
            .iter()
            .chain(&custom_types)
            .enumerate()
            .map(|(index, &type_id)| {
                (
                    type_id,
                    u32::try_from(index).expect("output type count fits the wire type space"),
                )
            })
            .collect();
        Self {
            builtins,
            custom_types,
            output_ids,
        }
    }

    pub(crate) fn builtins(&self) -> &[TypeId] {
        &self.builtins
    }

    pub(crate) fn custom_types(&self) -> &[TypeId] {
        &self.custom_types
    }

    pub(crate) fn output_id(&self, type_id: TypeId) -> u32 {
        *self
            .output_ids
            .get(&type_id)
            .expect("reachable output type has a projected identity")
    }

    pub(crate) fn contains(&self, type_id: TypeId) -> bool {
        self.output_ids.contains_key(&type_id)
    }
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
                    TypeShape::Variant(cases) => (
                        CaptureScopeKind::Variant,
                        cases
                            .iter()
                            .map(|(&name, &payload)| CaptureMember {
                                name,
                                kind: CaptureMemberKind::Case(payload),
                            })
                            .collect(),
                    ),
                    _ => continue,
                };

            let next_member_count = member_count + members.len();
            if next_member_count > MAX_MEMBERS {
                return Err(OutputSchemaError::Members(next_member_count));
            }
            let base = u16::try_from(member_count)
                .expect("member count was checked against the u16 format limit");
            member_count = next_member_count;
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
    collected_types: CollectedTypes,
    type_layout: OnceLock<OutputTypeLayout>,
    type_names: BTreeMap<TypeId, Symbol>,
    entrypoint_items: Vec<OutputItem>,
    capture_layout: CaptureLayout,
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
        let reachable_defs =
            deps.reachable_from(types.iter_entry_point_outputs().map(|(def_id, _)| def_id));
        let collected_types = CollectedTypes::collect(types, reachable_defs.iter());
        let capture_layout = CaptureLayout::build(types, &collected_types.custom)?;
        let mut type_names: BTreeMap<TypeId, Symbol> = types.iter_type_names().collect();
        // Leaf wrappers are structurally interned, so an unreachable definition
        // can share its TypeId with a reachable one. Reassert reachable nominal
        // names here: type reachability alone must never retain the dead name.
        for def_id in reachable_defs.iter() {
            let output = types.expect_def_output(def_id);
            if output == TYPE_VOID {
                continue;
            }
            type_names.insert(output, deps.def_name_sym(def_id));
        }
        let entrypoint_items = ItemCollector::new(types, deps, &type_names).collect();
        Ok(Self {
            types,
            deps,
            interner,
            collected_types,
            type_layout: OnceLock::new(),
            type_names,
            entrypoint_items,
            capture_layout,
        })
    }

    pub(crate) fn type_layout(&self) -> &OutputTypeLayout {
        self.type_layout.get_or_init(|| {
            OutputTypeLayout::build(
                &self.collected_types.builtins,
                self.collected_types.custom.clone(),
            )
        })
    }

    pub(crate) fn type_name_of(&self, type_id: TypeId) -> Option<Symbol> {
        self.type_names.get(&type_id).copied()
    }

    pub(crate) fn iter_type_names(&self) -> impl Iterator<Item = (TypeId, Symbol)> + '_ {
        self.type_names
            .iter()
            .map(|(&type_id, &name)| (type_id, name))
    }

    /// Public output items reachable from selectable definition outputs.
    ///
    /// Reachable fragment definitions still need capture slots and wire types
    /// for matching. A source target, however, must not publish an
    /// unspecialized fragment type merely because a scalar-specialized call
    /// uses the same matcher body.
    pub(crate) fn entrypoint_items(&self) -> &[OutputItem] {
        &self.entrypoint_items
    }

    pub(crate) fn layout(&self) -> &CaptureLayout {
        &self.capture_layout
    }

    pub(crate) fn dependency_analysis(&self) -> &DependencyAnalysis {
        self.deps
    }

    pub(crate) fn interner(&self) -> &Interner {
        self.interner
    }
}

struct ItemCollector<'a> {
    types: &'a TypeAnalysis,
    deps: &'a DependencyAnalysis,
    type_names: &'a BTreeMap<TypeId, Symbol>,
    declared_names: HashSet<Symbol>,
    walked_types: HashSet<TypeId>,
    items: Vec<OutputItem>,
}

impl<'a> ItemCollector<'a> {
    fn new(
        types: &'a TypeAnalysis,
        deps: &'a DependencyAnalysis,
        type_names: &'a BTreeMap<TypeId, Symbol>,
    ) -> Self {
        Self {
            types,
            deps,
            type_names,
            declared_names: HashSet::new(),
            walked_types: HashSet::new(),
            items: Vec::new(),
        }
    }

    fn collect(mut self) -> Vec<OutputItem> {
        for (def_id, output) in self.types.iter_entry_point_outputs() {
            let name = self.deps.def_name_sym(def_id);
            if output == TYPE_VOID {
                self.items.push(OutputItem::void_definition(name));
                continue;
            }
            self.add_item(name, output);
        }
        self.items
    }

    fn add_item(&mut self, name: Symbol, ty: TypeId) {
        if !self.declared_names.insert(name) {
            return;
        }

        let item = OutputItem::for_output(name, ty, self.types.expect_type_shape(ty));
        self.items.push(item);
        self.walk(ty);
    }

    fn walk(&mut self, ty: TypeId) {
        if !self.walked_types.insert(ty) {
            return;
        }

        match self.types.expect_type_shape(ty) {
            TypeShape::Struct(fields) => {
                for info in fields.values() {
                    self.collect_position(info.type_id);
                }
            }
            TypeShape::Variant(cases) => {
                for &payload in cases.values() {
                    if payload == TYPE_VOID {
                        continue;
                    }
                    let TypeShape::Struct(fields) = self.types.expect_type_shape(payload) else {
                        unreachable!("variant case payload is void or an anonymous struct");
                    };
                    for info in fields.values() {
                        self.collect_position(info.type_id);
                    }
                }
            }
            TypeShape::Array { element, .. } | TypeShape::Optional(element) => {
                self.collect_position(*element)
            }
            TypeShape::Ref(def_id) => self.collect_reference(*def_id),
            TypeShape::Void
            | TypeShape::Node
            | TypeShape::Text
            | TypeShape::Bool
            | TypeShape::Custom(_) => {}
        }
    }

    fn collect_position(&mut self, ty: TypeId) {
        match self.types.expect_type_shape(ty) {
            TypeShape::Struct(_) | TypeShape::Variant(_) => {
                let name = *self
                    .type_names
                    .get(&ty)
                    .expect("naming pass names every non-payload composite");
                self.add_item(name, ty);
            }
            TypeShape::Custom(name) => self.add_item(*name, ty),
            TypeShape::Array { .. } | TypeShape::Optional(_) => {
                let Some(&name) = self.type_names.get(&ty) else {
                    self.walk(ty);
                    return;
                };
                self.add_item(name, ty);
            }
            TypeShape::Ref(def_id) => self.collect_reference(*def_id),
            TypeShape::Node | TypeShape::Text | TypeShape::Bool => {}
            TypeShape::Void => unreachable!("void cannot appear in an output position"),
        }
    }

    fn collect_reference(&mut self, def_id: DefId) {
        let output = self.types.expect_def_output(def_id);
        if output == TYPE_VOID {
            return;
        }
        self.add_item(self.deps.def_name_sym(def_id), output);
    }
}

/// Custom types in the exact post-order the wire table historically used.
#[cfg(test)]
pub(super) fn collect_ordered_types(types: &TypeAnalysis) -> Vec<TypeId> {
    CollectedTypes::collect(types, types.iter_def_output().map(|(def_id, _)| def_id)).custom
}

#[derive(Clone, Debug)]
struct CollectedTypes {
    custom: Vec<TypeId>,
    builtins: HashSet<TypeId>,
}

impl CollectedTypes {
    fn collect(types: &TypeAnalysis, definitions: impl IntoIterator<Item = DefId>) -> Self {
        let mut collector = TypeCollector::new();
        for def_id in definitions {
            let type_id = types.expect_def_output(def_id);
            collector.collect(type_id, types);
            if !matches!(types.expect_type_shape(type_id), TypeShape::Ref(_)) {
                continue;
            }
            if collector.seen.insert(type_id) {
                collector.out.push(type_id);
            }
        }
        Self {
            custom: collector.out,
            builtins: collector.builtins,
        }
    }
}

struct TypeCollector {
    out: Vec<TypeId>,
    seen: HashSet<TypeId>,
    builtins: HashSet<TypeId>,
}

impl TypeCollector {
    fn new() -> Self {
        Self {
            out: Vec::new(),
            seen: HashSet::new(),
            builtins: HashSet::new(),
        }
    }

    fn collect(&mut self, type_id: TypeId, types: &TypeAnalysis) {
        if type_id.is_builtin() {
            self.builtins.insert(type_id);
            return;
        }
        if self.seen.contains(&type_id) {
            return;
        }
        let shape = types.expect_type_shape(type_id);
        if let TypeShape::Ref(def_id) = shape {
            let target = types.expect_def_output(*def_id);
            if target == TYPE_VOID {
                self.builtins.insert(TYPE_NODE);
                return;
            }
            self.collect(target, types);
            return;
        }

        self.seen.insert(type_id);
        if matches!(shape, TypeShape::Custom(_)) {
            self.builtins.insert(TYPE_NODE);
        }
        for child in shape.child_type_ids() {
            self.collect(child, types);
        }
        if matches!(
            shape,
            TypeShape::Struct(_)
                | TypeShape::Variant(_)
                | TypeShape::Array { .. }
                | TypeShape::Optional(_)
                | TypeShape::Custom(_)
        ) {
            self.out.push(type_id);
        }
    }
}
