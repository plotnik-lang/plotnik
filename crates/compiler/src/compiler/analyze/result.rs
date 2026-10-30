use std::collections::{BTreeMap, HashSet};

use indexmap::IndexMap;
use thiserror::Error;

use crate::compiler::analyze::AnalysisArtifacts;
use crate::compiler::analyze::refs::{DefinitionGraph, DefinitionReachability};
use crate::compiler::analyze::types::TypeAnalysis;
use crate::compiler::analyze::types::type_shape::{
    CasePayload, DefinitionOutput, RecordField, TYPE_BOOL, TYPE_NODE, TYPE_TEXT, TypeId, TypeShape,
};
use crate::compiler::ids::{DefId, ResultMemberId, ResultTypeId, TypeDeclId};
use crate::core::{Interner, Symbol};

const MAX_MEMBERS: usize = u16::MAX as usize;

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub(crate) enum ResultSchemaError {
    #[error("too many type members: {0} (max {MAX_MEMBERS})")]
    Members(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResultItemKind {
    Record,
    Variant,
    Alias,
    MatchOnlyDef,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ResultItem {
    pub(crate) name: Symbol,
    pub(crate) output: DefinitionOutput,
    pub(crate) kind: ResultItemKind,
}

impl ResultItem {
    fn for_result(name: Symbol, ty: TypeId, shape: &TypeShape) -> Self {
        let kind = match shape {
            TypeShape::Record(_) => ResultItemKind::Record,
            TypeShape::Variant(_) => ResultItemKind::Variant,
            _ => ResultItemKind::Alias,
        };
        Self {
            name,
            output: DefinitionOutput::Value(ty),
            kind,
        }
    }

    fn match_only_definition(name: Symbol) -> Self {
        Self {
            name,
            output: DefinitionOutput::MatchOnly,
            kind: ResultItemKind::MatchOnlyDef,
        }
    }

    pub(crate) fn is_composite(self) -> bool {
        matches!(self.kind, ResultItemKind::Record | ResultItemKind::Variant)
    }

    pub(crate) fn value_type(self) -> TypeId {
        self.output
            .value()
            .expect("value-bearing result item must have a value type")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum CaptureScopeKind {
    Record,
    Variant,
}

#[derive(Clone, Debug)]
pub(crate) struct PublicResultGroup {
    representative: TypeId,
    occurrences: Vec<TypeId>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum ResultTarget {
    Item(Symbol),
    Shape(TypeId),
    Payload(TypeId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CaptureMemberKind {
    Field(RecordField),
    Case(CasePayload),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CaptureMember {
    pub(crate) parent_type: TypeId,
    pub(crate) name: Symbol,
    pub(crate) kind: CaptureMemberKind,
}

#[derive(Clone, Debug)]
pub(crate) struct CaptureScope {
    kind: CaptureScopeKind,
    base: u16,
    members_by_name: BTreeMap<Symbol, ResultMemberId>,
}

impl CaptureScope {
    pub(crate) fn kind(&self) -> CaptureScopeKind {
        self.kind
    }

    pub(crate) fn base(&self) -> u16 {
        self.base
    }

    pub(crate) fn members(&self) -> impl ExactSizeIterator<Item = ResultMemberId> + '_ {
        self.members_by_name.values().copied()
    }

    pub(crate) fn member_named(&self, name: Symbol) -> Option<ResultMemberId> {
        self.members_by_name.get(&name).copied()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CaptureLayout {
    scopes: BTreeMap<TypeId, CaptureScope>,
    members: Vec<CaptureMember>,
}

#[derive(Clone, Debug)]
pub(crate) struct ResultTypeLayout {
    no_value: bool,
    types: IndexMap<TypeId, ResultTypeId>,
}

impl ResultTypeLayout {
    fn build(no_value: bool, used_builtins: &HashSet<TypeId>, value_types: Vec<TypeId>) -> Self {
        let ordered_types = [TYPE_NODE, TYPE_TEXT, TYPE_BOOL]
            .into_iter()
            .filter(|builtin| used_builtins.contains(builtin))
            .chain(value_types);
        let types = ordered_types
            .enumerate()
            .map(|(index, type_id)| {
                (
                    type_id,
                    ResultTypeId::from_raw(
                        u32::try_from(index + usize::from(no_value))
                            .expect("result type count fits the result type space"),
                    ),
                )
            })
            .collect();
        Self { no_value, types }
    }

    pub(crate) fn builtins(&self) -> impl Iterator<Item = TypeId> + '_ {
        self.types
            .keys()
            .copied()
            .take_while(|type_id| type_id.is_builtin())
    }

    pub(crate) fn has_no_value(&self) -> bool {
        self.no_value
    }

    pub(crate) fn no_value_output_id(&self) -> ResultTypeId {
        assert!(
            self.no_value,
            "result layout must include the no-value wire type"
        );
        ResultTypeId::from_raw(0)
    }

    pub(crate) fn value_types(&self) -> impl Iterator<Item = TypeId> + '_ {
        self.types
            .keys()
            .copied()
            .skip_while(|type_id| type_id.is_builtin())
    }

    pub(crate) fn output_id(&self, type_id: TypeId) -> ResultTypeId {
        *self
            .types
            .get(&type_id)
            .expect("reachable result type has a projected identity")
    }

    pub(crate) fn contains(&self, type_id: TypeId) -> bool {
        self.types.contains_key(&type_id)
    }
}

impl CaptureLayout {
    pub(super) fn build(
        types: &TypeAnalysis,
        ordered_types: &[TypeId],
    ) -> Result<Self, ResultSchemaError> {
        let mut scopes = BTreeMap::new();
        let mut all_members = Vec::new();
        for &type_id in ordered_types {
            let (kind, members): (CaptureScopeKind, Vec<(Symbol, CaptureMemberKind)>) =
                match types.expect_type_shape(type_id) {
                    TypeShape::Record(fields) => (
                        CaptureScopeKind::Record,
                        fields
                            .iter()
                            .map(|(&name, &info)| (name, CaptureMemberKind::Field(info)))
                            .collect(),
                    ),
                    TypeShape::Variant(cases) => (
                        CaptureScopeKind::Variant,
                        cases
                            .iter()
                            .map(|(&name, &payload)| (name, CaptureMemberKind::Case(payload)))
                            .collect(),
                    ),
                    _ => continue,
                };

            let member_count = all_members.len();
            let next_member_count = member_count + members.len();
            if next_member_count > MAX_MEMBERS {
                return Err(ResultSchemaError::Members(next_member_count));
            }
            let base = u16::try_from(member_count)
                .expect("member count was checked against the u16 format limit");
            let mut members_by_name = BTreeMap::new();
            for (relative_index, (name, kind)) in members.into_iter().enumerate() {
                let relative_index = u16::try_from(relative_index)
                    .expect("capture scope member count fits the result member space");
                let id = ResultMemberId::from_raw(
                    base.checked_add(relative_index)
                        .expect("capture layout validates the result member space"),
                );
                all_members.push(CaptureMember {
                    parent_type: type_id,
                    name,
                    kind,
                });
                members_by_name.insert(name, id);
            }
            scopes.insert(
                type_id,
                CaptureScope {
                    kind,
                    base,
                    members_by_name,
                },
            );
        }

        Ok(Self {
            scopes,
            members: all_members,
        })
    }

    pub(crate) fn scope(&self, type_id: TypeId) -> Option<&CaptureScope> {
        self.scopes.get(&type_id)
    }

    pub(crate) fn member_id(&self, parent_type: TypeId, name: Symbol) -> Option<ResultMemberId> {
        self.scope(parent_type)?.member_named(name)
    }

    pub(crate) fn member(&self, id: ResultMemberId) -> Option<&CaptureMember> {
        self.members.get(id.index())
    }

    pub(crate) fn expect_member(&self, id: ResultMemberId) -> &CaptureMember {
        self.member(id)
            .expect("result member id belongs to the compiled result model")
    }

    pub(crate) fn member_count(&self) -> usize {
        self.members.len()
    }
}

impl PublicResultGroup {
    fn collect(
        items: &[ResultItem],
        types: &TypeAnalysis,
        layout: &CaptureLayout,
        roots: impl IntoIterator<Item = (Symbol, TypeId)>,
    ) -> BTreeMap<Symbol, Self> {
        let mut groups = BTreeMap::new();
        let mut pending = Vec::new();
        for item in items.iter().copied().filter(|item| item.is_composite()) {
            let group = Self {
                representative: item.value_type(),
                occurrences: Vec::new(),
            };
            let previous = groups.insert(item.name, group);
            assert!(previous.is_none(), "public result item names are unique");
        }
        let item_names = items.iter().map(|item| item.name).collect::<HashSet<_>>();
        for (name, occurrence) in roots {
            if item_names.contains(&name) {
                pending.push((ResultTarget::Item(name), occurrence));
            }
        }
        for (type_id, name) in types.iter_named_types() {
            if groups.contains_key(&name) && layout.scope(type_id).is_some() {
                pending.push((ResultTarget::Item(name), type_id));
            }
        }

        let mut visited = HashSet::new();
        while let Some((target, occurrence)) = pending.pop() {
            let occurrence = types.resolve_underlying_type_id(occurrence);
            if !visited.insert((target, occurrence)) {
                continue;
            }
            let representative = match target {
                ResultTarget::Item(name) => {
                    let body = types
                        .public_type(name)
                        .value()
                        .expect("a public result binding produces a value");
                    if let Some(group) = groups.get_mut(&name) {
                        group.add_occurrence(types, layout, occurrence);
                    }
                    if matches!(types.expect_type_shape(body), TypeShape::Ref(_)) {
                        pending.push((ResultTarget::Shape(body), occurrence));
                        continue;
                    }
                    body
                }
                ResultTarget::Shape(ty) => {
                    if let Some(name) = types.public_name(ty) {
                        pending.push((ResultTarget::Item(name), occurrence));
                        continue;
                    }
                    ty
                }
                ResultTarget::Payload(ty) => ty,
            };
            match (
                types.expect_type_shape(representative),
                types.expect_type_shape(occurrence),
            ) {
                (TypeShape::Record(fields), TypeShape::Record(actual)) => {
                    for (&name, field) in fields {
                        let actual = actual.get(&name).expect("bound records share field names");
                        pending.push((ResultTarget::Shape(field.final_type), actual.final_type));
                    }
                }
                (TypeShape::Variant(cases), TypeShape::Variant(actual)) => {
                    for (&name, payload) in cases {
                        let actual = actual.get(&name).expect("bound variants share case names");
                        if let Some(payload) = payload.type_id() {
                            pending.push((
                                ResultTarget::Payload(payload),
                                actual
                                    .type_id()
                                    .expect("bound cases share payload presence"),
                            ));
                        }
                    }
                }
                (TypeShape::Option(inner), TypeShape::Option(actual))
                | (
                    TypeShape::List { element: inner, .. },
                    TypeShape::List {
                        element: actual, ..
                    },
                ) => {
                    pending.push((ResultTarget::Shape(*inner), *actual));
                }
                (TypeShape::Node, TypeShape::Node)
                | (TypeShape::Text, TypeShape::Text)
                | (TypeShape::Bool, TypeShape::Bool) => {}
                _ => unreachable!("validated public and concrete result shapes agree"),
            }
        }
        for group in groups.values_mut() {
            group.finish();
        }
        groups
    }

    fn add_occurrence(&mut self, types: &TypeAnalysis, layout: &CaptureLayout, type_id: TypeId) {
        layout
            .scope(type_id)
            .expect("public result occurrence has a capture scope");
        assert!(
            types.types_structurally_equal(self.representative, type_id),
            "one public result name has one structural shape"
        );
        self.occurrences.push(type_id);
    }

    fn finish(&mut self) {
        self.occurrences.sort_unstable();
        self.occurrences.dedup();
        assert!(
            !self.occurrences.is_empty(),
            "public result has concrete bindings"
        );
    }

    pub(crate) fn representative(&self) -> TypeId {
        self.representative
    }

    pub(crate) fn occurrences(&self) -> &[TypeId] {
        &self.occurrences
    }
}

#[derive(Clone)]
pub(crate) struct ResultModel {
    reachable_defs: DefinitionReachability,
    type_layout: ResultTypeLayout,
    type_name_bindings: Vec<TypeNameBinding>,
    entrypoint_items: Vec<ResultItem>,
    capture_layout: CaptureLayout,
    public_result_groups: BTreeMap<Symbol, PublicResultGroup>,
}

#[derive(Clone, Copy)]
pub(crate) struct ResultSchema<'a> {
    pub(crate) types: &'a TypeAnalysis,
    pub(crate) definitions: &'a DefinitionGraph,
    pub(crate) interner: &'a Interner,
    model: &'a ResultModel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TypeNameBinding {
    pub(crate) name: Symbol,
    pub(crate) type_id: TypeId,
}

impl ResultModel {
    pub(crate) fn from_artifacts(
        artifacts: AnalysisArtifacts<'_>,
    ) -> Result<Self, ResultSchemaError> {
        let types = artifacts.type_analysis;
        let definitions = artifacts.definitions;
        let roots = artifacts.iter_entrypoint_outputs().map(|(id, _)| id).chain(
            artifacts
                .expression
                .into_iter()
                .flat_map(|body| definitions.expression_dependencies(body.node())),
        );
        let reachable_defs = definitions.reachable_from(roots);
        let collected_types = CollectedTypes::collect(
            types,
            reachable_defs
                .iter()
                .map(|id| types.expect_def_output(id))
                .chain(artifacts.expression_output()),
        );
        let capture_layout = CaptureLayout::build(types, &collected_types.value_types)?;
        let type_layout = ResultTypeLayout::build(
            collected_types.no_value,
            &collected_types.builtins,
            collected_types.value_types,
        );
        let mut type_name_bindings = types
            .iter_named_types()
            .map(|(type_id, name)| TypeNameBinding { name, type_id })
            .collect::<Vec<_>>();
        for def_id in reachable_defs.iter() {
            let Some(body) = types.expect_def_output(def_id).value() else {
                continue;
            };
            type_name_bindings.push(TypeNameBinding {
                name: definitions.definition(def_id).name(),
                type_id: body,
            });
        }
        type_name_bindings.sort_by_key(|binding| (binding.type_id, binding.name));
        type_name_bindings.dedup();
        let entrypoint_items =
            ItemCollector::new(types, definitions).collect(artifacts.iter_entrypoint_outputs());
        let occurrences = reachable_defs.iter().filter_map(|definition| {
            types
                .expect_def_output(definition)
                .value()
                .map(|body| (definitions.definition(definition).name(), body))
        });
        let public_result_groups =
            PublicResultGroup::collect(&entrypoint_items, types, &capture_layout, occurrences);
        Ok(Self {
            reachable_defs,
            type_layout,
            type_name_bindings,
            entrypoint_items,
            capture_layout,
            public_result_groups,
        })
    }

    pub(crate) fn schema<'a>(&'a self, artifacts: AnalysisArtifacts<'a>) -> ResultSchema<'a> {
        ResultSchema::new(
            self,
            artifacts.type_analysis,
            artifacts.definitions,
            artifacts.interner,
        )
    }

    pub(crate) fn layout(&self) -> &CaptureLayout {
        &self.capture_layout
    }

    pub(crate) fn reachable_defs(&self) -> &DefinitionReachability {
        &self.reachable_defs
    }

    pub(crate) fn public_result_group(&self, item: ResultItem) -> &PublicResultGroup {
        let group = self
            .public_result_groups
            .get(&item.name)
            .expect("composite result item has a public group");
        assert_eq!(
            group.representative,
            item.value_type(),
            "public group owns the item representative"
        );
        group
    }
}

impl<'a> ResultSchema<'a> {
    pub(crate) fn new(
        model: &'a ResultModel,
        types: &'a TypeAnalysis,
        definitions: &'a DefinitionGraph,
        interner: &'a Interner,
    ) -> Self {
        Self {
            types,
            definitions,
            interner,
            model,
        }
    }

    pub(crate) fn type_layout(&self) -> &ResultTypeLayout {
        &self.model.type_layout
    }

    pub(crate) fn iter_type_name_bindings(&self) -> impl Iterator<Item = TypeNameBinding> + '_ {
        self.model.type_name_bindings.iter().copied()
    }

    pub(crate) fn entrypoint_items(&self) -> &[ResultItem] {
        &self.model.entrypoint_items
    }

    pub(crate) fn layout(&self) -> &CaptureLayout {
        &self.model.capture_layout
    }

    pub(crate) fn public_result_group(&self, item: ResultItem) -> &PublicResultGroup {
        self.model.public_result_group(item)
    }
}

struct ItemCollector<'a> {
    types: &'a TypeAnalysis,
    definitions: &'a DefinitionGraph,
    declared_names: HashSet<Symbol>,
    walked_types: HashSet<TypeId>,
    items: Vec<ResultItem>,
}

impl<'a> ItemCollector<'a> {
    fn new(types: &'a TypeAnalysis, definitions: &'a DefinitionGraph) -> Self {
        Self {
            types,
            definitions,
            declared_names: HashSet::new(),
            walked_types: HashSet::new(),
            items: Vec::new(),
        }
    }

    fn collect(
        mut self,
        entrypoints: impl IntoIterator<Item = (DefId, DefinitionOutput)>,
    ) -> Vec<ResultItem> {
        for (def_id, output) in entrypoints {
            let name = self.definitions.definition(def_id).name();
            match output {
                DefinitionOutput::MatchOnly => {
                    self.items.push(ResultItem::match_only_definition(name));
                }
                DefinitionOutput::Value(_) => self.add_item(name),
            }
        }
        self.items
    }

    fn add_item(&mut self, name: Symbol) {
        if !self.declared_names.insert(name) {
            return;
        }

        let ty = self
            .types
            .public_type(name)
            .value()
            .expect("value-bearing public item has a representative");
        let item = ResultItem::for_result(name, ty, self.types.expect_type_shape(ty));
        self.items.push(item);
        self.walk(ty);
    }

    fn walk(&mut self, ty: TypeId) {
        if !self.walked_types.insert(ty) {
            return;
        }

        match self.types.expect_type_shape(ty) {
            TypeShape::Record(fields) => {
                for info in fields.values() {
                    self.collect_position(info.final_type);
                }
            }
            TypeShape::Variant(cases) => {
                for payload in cases.values() {
                    let Some(payload) = payload.type_id() else {
                        continue;
                    };
                    let TypeShape::Record(fields) = self.types.expect_type_shape(payload) else {
                        unreachable!("variant case has no payload or an anonymous record payload");
                    };
                    for info in fields.values() {
                        self.collect_position(info.final_type);
                    }
                }
            }
            TypeShape::List { element, .. } | TypeShape::Option(element) => {
                self.collect_position(*element)
            }
            TypeShape::Ref(declaration) => self.collect_reference(*declaration),
            TypeShape::Node | TypeShape::Text | TypeShape::Bool => {}
        }
    }

    fn collect_position(&mut self, ty: TypeId) {
        match self.types.expect_type_shape(ty) {
            TypeShape::Record(_) | TypeShape::Variant(_) => {
                let name = self
                    .types
                    .type_name_of(ty)
                    .expect("naming pass names every non-payload composite");
                self.add_item(name);
            }
            TypeShape::List { .. } | TypeShape::Option(_) => {
                let Some(name) = self.types.type_name_of(ty) else {
                    self.walk(ty);
                    return;
                };
                self.add_item(name);
            }
            TypeShape::Ref(declaration) => self.collect_reference(*declaration),
            TypeShape::Node | TypeShape::Text | TypeShape::Bool => {}
        }
    }

    fn collect_reference(&mut self, declaration: TypeDeclId) {
        let Some(declaration) = self.types.declaration(declaration) else {
            return;
        };
        self.add_item(declaration.name);
    }
}

#[derive(Clone, Debug)]
struct CollectedTypes {
    value_types: Vec<TypeId>,
    builtins: HashSet<TypeId>,
    no_value: bool,
}

impl CollectedTypes {
    fn collect(types: &TypeAnalysis, outputs: impl IntoIterator<Item = DefinitionOutput>) -> Self {
        let mut collector = TypeCollector::new();
        let mut no_value = false;
        for output in outputs {
            let Some(type_id) = output.value() else {
                no_value = true;
                continue;
            };
            collector.collect(type_id, types);
            if !matches!(types.expect_type_shape(type_id), TypeShape::Ref(_)) {
                continue;
            }
            if collector.seen.insert(type_id) {
                collector.out.push(type_id);
            }
        }
        Self {
            value_types: collector.out,
            builtins: collector.builtins,
            no_value: no_value || collector.no_value,
        }
    }
}

struct TypeCollector {
    out: Vec<TypeId>,
    seen: HashSet<TypeId>,
    builtins: HashSet<TypeId>,
    no_value: bool,
}

impl TypeCollector {
    fn new() -> Self {
        Self {
            out: Vec::new(),
            seen: HashSet::new(),
            builtins: HashSet::new(),
            no_value: false,
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
        if let TypeShape::Variant(cases) = shape
            && cases.values().any(|payload| payload.type_id().is_none())
        {
            self.no_value = true;
        }
        if let TypeShape::Ref(declaration) = shape {
            let Some(declaration) = types.declaration(*declaration) else {
                self.builtins.insert(TYPE_NODE);
                return;
            };
            self.collect(declaration.body, types);
            if types.declaration_definition(declaration.id).is_none() && self.seen.insert(type_id) {
                self.out.push(type_id);
            }
            return;
        }

        self.seen.insert(type_id);
        for child in shape.child_type_ids() {
            self.collect(child, types);
        }
        if matches!(
            shape,
            TypeShape::Record(_)
                | TypeShape::Variant(_)
                | TypeShape::List { .. }
                | TypeShape::Option(_)
        ) {
            self.out.push(type_id);
        }
    }
}
