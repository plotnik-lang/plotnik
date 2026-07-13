//! Target-neutral typed replay plan.
//!
//! Capture traces name struct fields and variant cases with absolute layout
//! slots. This plan resolves every nominal twin to the complete set of slots
//! it may produce and turns output types into a small value-reading algebra.
//! Backends choose identifiers, error syntax, recursion representation, and
//! construction syntax without walking analysis types or rebuilding tables.

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::compiler::analyze::output::{CaptureLayout, OutputItem, OutputItemKind, OutputSchema};
use crate::compiler::analyze::types::TypeAnalysis;
use crate::compiler::analyze::types::type_shape::{FieldInfo, TYPE_VOID, TypeId, TypeShape};
use crate::core::Symbol;

#[derive(Clone, Debug)]
pub(crate) struct ReplayPlan {
    items: Vec<ReplayItem>,
    items_by_name: HashMap<Symbol, usize>,
}

impl ReplayPlan {
    pub(super) fn build(schema: &OutputSchema<'_>) -> Self {
        let builder = ReplayPlanBuilder { schema };
        let items = schema
            .entrypoint_items()
            .iter()
            .map(|item| builder.item(*item))
            .collect::<Vec<_>>();
        let items_by_name = items
            .iter()
            .enumerate()
            .map(|(index, item)| (item.name, index))
            .collect();
        Self {
            items,
            items_by_name,
        }
    }

    pub(crate) fn items(&self) -> &[ReplayItem] {
        &self.items
    }

    pub(crate) fn item(&self, name: Symbol) -> &ReplayItem {
        let index = self
            .items_by_name
            .get(&name)
            .expect("every replay target declares an output item");
        &self.items[*index]
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ReplayItem {
    pub(crate) name: Symbol,
    pub(crate) ty: TypeId,
    pub(crate) kind: ReplayItemKind,
    /// This reader is a recursive definition's dynamic depth boundary.
    pub(crate) enters_depth: bool,
    /// This reader or one it calls can trip the replay-depth limit.
    pub(crate) fallible: bool,
}

impl ReplayItem {
    pub(crate) fn has_reader(&self) -> bool {
        !matches!(self.kind, ReplayItemKind::VoidDefinition)
    }
}

#[derive(Clone, Debug)]
pub(crate) enum ReplayItemKind {
    Struct(ReplayScopePlan),
    Variant(Vec<ReplayCasePlan>),
    Alias(ReplayValuePlan),
    VoidDefinition,
}

#[derive(Clone, Debug)]
pub(crate) struct ReplayScopePlan {
    pub(crate) fields: Vec<ReplayFieldPlan>,
}

#[derive(Clone, Debug)]
pub(crate) struct ReplayFieldPlan {
    pub(crate) name: Symbol,
    /// Every absolute `Set` slot this field may use across nominal twins.
    pub(crate) indices: Vec<u16>,
    pub(crate) value: ReplayValuePlan,
}

#[derive(Clone, Debug)]
pub(crate) struct ReplayCasePlan {
    pub(crate) name: Symbol,
    /// Every absolute `VariantOpen` slot this case may use across twins.
    pub(crate) indices: Vec<u16>,
    pub(crate) payload: Option<ReplayScopePlan>,
}

#[derive(Clone, Debug)]
pub(crate) enum ReplayValuePlan {
    Node,
    Text,
    Bool,
    Nullable(Box<ReplayValuePlan>),
    Array(Box<ReplayValuePlan>),
    Read {
        item: Symbol,
        /// Analysis type at this read position. It preserves whether the
        /// position is a direct nominal value or a definition-reference
        /// occurrence without prescribing a target's representation.
        source: TypeId,
    },
}

struct ReplayPlanBuilder<'p, 'a> {
    schema: &'p OutputSchema<'a>,
}

impl ReplayPlanBuilder<'_, '_> {
    fn item(&self, item: OutputItem) -> ReplayItem {
        let kind = match item.kind {
            OutputItemKind::Struct => {
                let TypeShape::Struct(fields) = self.schema.types.expect_type_shape(item.ty) else {
                    unreachable!("struct output item has a struct shape");
                };
                let twins = collect_twins(self.schema.types, self.schema.layout(), item);
                ReplayItemKind::Struct(self.scope(fields.iter(), &twins))
            }
            OutputItemKind::Variant => ReplayItemKind::Variant(self.variant_cases(item)),
            OutputItemKind::Alias => ReplayItemKind::Alias(self.value(item.ty)),
            OutputItemKind::VoidDef => ReplayItemKind::VoidDefinition,
        };
        ReplayItem {
            name: item.name,
            ty: item.ty,
            kind,
            enters_depth: self.item_enters_depth(item.name),
            fallible: self.item_is_fallible(item),
        }
    }

    fn variant_cases(&self, item: OutputItem) -> Vec<ReplayCasePlan> {
        let TypeShape::Variant(cases) = self.schema.types.expect_type_shape(item.ty) else {
            unreachable!("variant output item has a variant shape");
        };
        let twins = collect_twins(self.schema.types, self.schema.layout(), item);
        cases
            .iter()
            .enumerate()
            .map(|(index, (&name, &payload))| {
                let payload = if payload == TYPE_VOID {
                    None
                } else {
                    let TypeShape::Struct(fields) = self.schema.types.expect_type_shape(payload)
                    else {
                        unreachable!("variant case payload is void or an anonymous struct");
                    };
                    let payloads = payload_twins(self.schema.types, &twins, index);
                    Some(self.scope(fields.iter(), &payloads))
                };
                ReplayCasePlan {
                    name,
                    indices: member_indices(self.schema.layout(), &twins, index),
                    payload,
                }
            })
            .collect()
    }

    fn scope<'b>(
        &self,
        fields: impl Iterator<Item = (&'b Symbol, &'b FieldInfo)>,
        twins: &[TypeId],
    ) -> ReplayScopePlan {
        ReplayScopePlan {
            fields: fields
                .enumerate()
                .map(|(index, (&name, info))| {
                    let value = self.value(info.type_id);
                    ReplayFieldPlan {
                        name,
                        indices: member_indices(self.schema.layout(), twins, index),
                        value: if info.optional {
                            ReplayValuePlan::Nullable(Box::new(value))
                        } else {
                            value
                        },
                    }
                })
                .collect(),
        }
    }

    fn value(&self, ty: TypeId) -> ReplayValuePlan {
        match self.schema.types.expect_type_shape(ty) {
            TypeShape::Node | TypeShape::Custom(_) => ReplayValuePlan::Node,
            TypeShape::Text => ReplayValuePlan::Text,
            TypeShape::Bool => ReplayValuePlan::Bool,
            TypeShape::Optional(inner) => ReplayValuePlan::Nullable(Box::new(self.value(*inner))),
            TypeShape::Array { element, .. } => {
                ReplayValuePlan::Array(Box::new(self.value(*element)))
            }
            TypeShape::Struct(_) | TypeShape::Variant(_) => ReplayValuePlan::Read {
                item: self
                    .schema
                    .type_name_of(ty)
                    .expect("naming pass names every non-payload composite"),
                source: ty,
            },
            TypeShape::Ref(definition) => {
                let target = self.schema.types.expect_def_output(*definition);
                if target == TYPE_VOID {
                    return ReplayValuePlan::Node;
                }
                ReplayValuePlan::Read {
                    item: self.schema.deps.def_name_sym(*definition),
                    source: ty,
                }
            }
            TypeShape::Void => unreachable!("void cannot appear in an output position"),
        }
    }

    fn item_enters_depth(&self, name: Symbol) -> bool {
        self.schema
            .deps
            .def_id_for_sym(name)
            .is_some_and(|definition| self.schema.deps.is_recursive_def(definition))
    }

    fn item_is_fallible(&self, item: OutputItem) -> bool {
        self.item_enters_depth(item.name)
            || self.type_reaches_recursive_ref(item.ty, &mut HashSet::new())
    }

    fn type_reaches_recursive_ref(&self, ty: TypeId, seen: &mut HashSet<TypeId>) -> bool {
        if !seen.insert(ty) {
            return false;
        }
        if let Some(name) = self.schema.type_name_of(ty)
            && self.item_enters_depth(name)
        {
            return true;
        }
        match self.schema.types.expect_type_shape(ty) {
            TypeShape::Ref(definition) => {
                let target = self.schema.types.expect_def_output(*definition);
                if target == TYPE_VOID {
                    return false;
                }
                self.schema.deps.is_recursive_def(*definition)
                    || self.type_reaches_recursive_ref(target, seen)
            }
            shape => shape
                .child_type_ids()
                .any(|child| self.type_reaches_recursive_ref(child, seen)),
        }
    }
}

/// Every table-reachable analysis type sharing this item's name and shape
/// kind. Structural identity is enforced upstream; twins differ only in
/// their member-run offsets.
fn collect_twins(types: &TypeAnalysis, layout: &CaptureLayout, item: OutputItem) -> Vec<TypeId> {
    let wants_struct = item.is_struct();
    let mut twins = BTreeSet::new();
    for (ty, name) in types.iter_type_names() {
        if name != item.name {
            continue;
        }
        let matches_kind = match types.expect_type_shape(ty) {
            TypeShape::Struct(_) => wants_struct,
            TypeShape::Variant(_) => !wants_struct,
            _ => false,
        };
        if !matches_kind || layout.member_base(ty).is_none() {
            continue;
        }
        twins.insert(ty);
    }
    twins.insert(item.ty);
    twins.into_iter().collect()
}

/// Case `index`'s payload struct across every twin variant type.
fn payload_twins(types: &TypeAnalysis, twins: &[TypeId], index: usize) -> Vec<TypeId> {
    twins
        .iter()
        .map(|&ty| {
            let TypeShape::Variant(cases) = types.expect_type_shape(ty) else {
                unreachable!("variant twins share the variant shape");
            };
            *cases
                .values()
                .nth(index)
                .expect("twins share the case list")
        })
        .collect()
}

/// Absolute member indices for one field/case position across all twins.
fn member_indices(layout: &CaptureLayout, twins: &[TypeId], index: usize) -> Vec<u16> {
    let relative = u16::try_from(index).expect("capture scope member count fits u16");
    let mut indices = twins
        .iter()
        .map(|&ty| {
            layout
                .scope(ty)
                .expect("twins are collected layout-present")
                .absolute_index(relative)
        })
        .collect::<Vec<_>>();
    indices.sort_unstable();
    indices.dedup();
    indices
}
