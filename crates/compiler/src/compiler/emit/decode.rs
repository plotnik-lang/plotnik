use indexmap::IndexMap;

use crate::compiler::analyze::result::{
    CaptureLayout, CaptureMemberKind, PublicResultGroup, ResultItem, ResultItemKind, ResultSchema,
};
use crate::compiler::analyze::types::type_shape::{DefinitionOutput, TypeId, TypeShape};
use crate::compiler::ids::ResultMemberId;
use crate::core::Symbol;

#[derive(Clone, Debug)]
pub(crate) struct ResultDecodePlan {
    items: IndexMap<Symbol, DecodeItem>,
}

impl ResultDecodePlan {
    pub(crate) fn build(schema: &ResultSchema<'_>) -> Self {
        let builder = DecodePlanBuilder { schema };
        let items = schema
            .entrypoint_items()
            .iter()
            .map(|item| {
                let item = builder.item(*item);
                (item.name, item)
            })
            .collect();
        Self { items }
    }

    pub(crate) fn items(&self) -> impl Iterator<Item = &DecodeItem> {
        self.items.values()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct DecodeItem {
    pub(crate) name: Symbol,
    pub(crate) output: DefinitionOutput,
    pub(crate) kind: DecodeItemKind,
}

#[derive(Clone, Debug)]
pub(crate) enum DecodeItemKind {
    Record(DecodeScope),
    Variant(Vec<DecodeCase>),
    Alias(DecodeValue),
    MatchOnlyDefinition,
}

#[derive(Clone, Debug)]
pub(crate) struct DecodeScope {
    pub(crate) fields: Vec<DecodeField>,
}

#[derive(Clone, Debug)]
pub(crate) struct DecodeField {
    pub(crate) name: Symbol,
    pub(crate) indices: Vec<ResultMemberId>,
    pub(crate) value: DecodeValue,
}

#[derive(Clone, Debug)]
pub(crate) struct DecodeCase {
    pub(crate) name: Symbol,
    pub(crate) indices: Vec<ResultMemberId>,
    pub(crate) payload: Option<DecodeScope>,
}

#[derive(Clone, Debug)]
pub(crate) enum DecodeValue {
    Node,
    Text,
    Bool,
    Option(Box<DecodeValue>),
    List {
        element: Box<DecodeValue>,
        minimum: bool,
    },
    Nested {
        item: Symbol,
        source_type: TypeId,
    },
}

struct DecodePlanBuilder<'p, 'a> {
    schema: &'p ResultSchema<'a>,
}

impl DecodePlanBuilder<'_, '_> {
    fn item(&self, item: ResultItem) -> DecodeItem {
        let kind = match item.kind {
            ResultItemKind::Record => {
                let group = self.schema.public_result_group(item);
                DecodeItemKind::Record(self.scope(group.representative(), group.occurrences()))
            }
            ResultItemKind::Variant => {
                let group = self.schema.public_result_group(item);
                DecodeItemKind::Variant(self.variant_cases(group))
            }
            ResultItemKind::Alias => DecodeItemKind::Alias(self.value(item.value_type())),
            ResultItemKind::MatchOnlyDef => DecodeItemKind::MatchOnlyDefinition,
        };
        DecodeItem {
            name: item.name,
            output: item.output,
            kind,
        }
    }

    fn variant_cases(&self, group: &PublicResultGroup) -> Vec<DecodeCase> {
        let layout = self.schema.layout();
        let scope = layout
            .scope(group.representative())
            .expect("variant result item has a capture scope");
        scope
            .members()
            .enumerate()
            .map(|(index, member)| {
                let descriptor = layout.expect_member(member);
                let CaptureMemberKind::Case(payload) = descriptor.kind else {
                    unreachable!("variant result scope contains only cases");
                };
                let payload = payload.type_id().map(|payload| {
                    let payloads = payload_occurrences(layout, group.occurrences(), index);
                    self.scope(payload, &payloads)
                });
                DecodeCase {
                    name: descriptor.name,
                    indices: member_indices(layout, group.occurrences(), index),
                    payload,
                }
            })
            .collect()
    }

    fn scope(&self, type_id: TypeId, twins: &[TypeId]) -> DecodeScope {
        let layout = self.schema.layout();
        let scope = layout
            .scope(type_id)
            .expect("record decode scope has a capture layout scope");
        DecodeScope {
            fields: scope
                .members()
                .enumerate()
                .map(|(index, member)| {
                    let descriptor = layout.expect_member(member);
                    let CaptureMemberKind::Field(info) = descriptor.kind else {
                        unreachable!("record result scope contains only fields");
                    };
                    DecodeField {
                        name: descriptor.name,
                        indices: member_indices(layout, twins, index),
                        value: self.value(info.final_type),
                    }
                })
                .collect(),
        }
    }

    fn value(&self, ty: TypeId) -> DecodeValue {
        match self.schema.types.expect_type_shape(ty) {
            TypeShape::Node => DecodeValue::Node,
            TypeShape::Text => DecodeValue::Text,
            TypeShape::Bool => DecodeValue::Bool,
            TypeShape::Option(inner) => DecodeValue::Option(Box::new(self.value(*inner))),
            TypeShape::List { element, minimum } => DecodeValue::List {
                element: Box::new(self.value(*element)),
                minimum: matches!(
                    minimum,
                    crate::compiler::analyze::types::type_shape::ListMinimum::One
                ),
            },
            TypeShape::Record(_) | TypeShape::Variant(_) => DecodeValue::Nested {
                item: self
                    .schema
                    .types
                    .type_name_of(ty)
                    .expect("naming pass names every non-payload composite"),
                source_type: ty,
            },
            TypeShape::Ref(declaration) => {
                if self.schema.types.declaration_body(*declaration).is_none() {
                    return DecodeValue::Node;
                }
                DecodeValue::Nested {
                    item: self.schema.types.declaration_name(*declaration),
                    source_type: ty,
                }
            }
        }
    }
}

fn payload_occurrences(
    layout: &CaptureLayout,
    occurrences: &[TypeId],
    index: usize,
) -> Vec<TypeId> {
    let relative = u16::try_from(index).expect("capture scope member count fits u16");
    occurrences
        .iter()
        .map(|&ty| {
            let member = layout
                .scope(ty)
                .and_then(|scope| scope.member_id(relative))
                .expect("variant twins share the case list");
            let CaptureMemberKind::Case(payload) = layout.expect_member(member).kind else {
                unreachable!("variant twin scopes contain only cases");
            };
            payload
                .type_id()
                .expect("payload twins belong to a payload-bearing case")
        })
        .collect()
}

fn member_indices(
    layout: &CaptureLayout,
    occurrences: &[TypeId],
    index: usize,
) -> Vec<ResultMemberId> {
    let relative = u16::try_from(index).expect("capture scope member count fits u16");
    let mut indices = occurrences
        .iter()
        .map(|&ty| {
            layout
                .scope(ty)
                .expect("twins are collected layout-present")
                .member_id(relative)
                .expect("twins share the member list")
        })
        .collect::<Vec<_>>();
    indices.sort_unstable();
    indices.dedup();
    indices
}
