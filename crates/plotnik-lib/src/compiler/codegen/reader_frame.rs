//! Conservative native-stack estimate for generated typed replay readers.
//!
//! Rust does not expose final stack maps during source generation. This model
//! counts the locals emitted for each replay shape; `plotnik-rt` adds runtime
//! padding when it converts the largest frame into an automatic depth limit.

use std::collections::{BTreeMap, HashSet};

use crate::compiler::analyze::types::TypeAnalysis;
use crate::compiler::analyze::types::type_shape::{FieldInfo, TYPE_VOID, TypeId, TypeShape};
use crate::compiler::codegen::plan::{ReplayItem, ReplayPlan};
use crate::compiler::typegen::rust::{TypeContext, TypeModel};
use crate::core::Symbol;

const WORD_BYTES: u64 = 8;
const NODE_VALUE_BYTES: u64 = 48;
const VEC_VALUE_BYTES: u64 = 24;
const OPTION_TAG_BYTES: u64 = 8;
const READER_FRAME_BASE_BYTES: u64 = 128;

// Compiler-only builds deliberately omit tree-sitter. Workspace/VM builds
// enable the runtime node type and turn representation drift into an error.
#[cfg(feature = "vm")]
const _: () = assert!(
    NODE_VALUE_BYTES >= std::mem::size_of::<plotnik_rt::Node<'static>>() as u64,
    "reader-frame Node estimate must cover plotnik-rt::Node"
);

pub(super) struct ReaderFrameEstimator<'m, 'a> {
    model: &'m TypeModel<'a>,
    types: &'a TypeAnalysis,
    replay: &'a ReplayPlan,
}

impl<'m, 'a> ReaderFrameEstimator<'m, 'a> {
    pub(super) fn new(model: &'m TypeModel<'a>, replay: &'a ReplayPlan) -> Self {
        Self {
            model,
            types: model.schema().types,
            replay,
        }
    }

    pub(super) fn max_bytes(&self) -> u64 {
        self.replay
            .items()
            .iter()
            .filter(|item| item.has_reader())
            .map(|item| self.reader_frame_bytes(item))
            .max()
            .unwrap_or(READER_FRAME_BASE_BYTES)
    }

    fn reader_frame_bytes(&self, item: &ReplayItem) -> u64 {
        let guard_bytes = if item.fallible { WORD_BYTES } else { 0 };
        let local_bytes = match self.types.expect_type_shape(item.ty) {
            TypeShape::Struct(fields) => self.field_scope_frame_bytes(item.ty, fields),
            TypeShape::Enum(variants) => variants
                .values()
                .map(|&payload| self.enum_payload_frame_bytes(item.ty, payload))
                .max()
                .unwrap_or(0),
            TypeShape::Void => 0,
            _ => self.value_temp_bytes(item.ty, TypeContext::item(item.ty)),
        };

        READER_FRAME_BASE_BYTES
            .saturating_add(guard_bytes)
            .saturating_add(local_bytes)
    }

    fn enum_payload_frame_bytes(&self, owner: TypeId, payload: TypeId) -> u64 {
        if payload == TYPE_VOID {
            return 0;
        }
        let TypeShape::Struct(fields) = self.types.expect_type_shape(payload) else {
            unreachable!("enum variant payload is void or an anonymous struct");
        };
        self.field_scope_frame_bytes(owner, fields)
    }

    fn field_scope_frame_bytes(&self, owner: TypeId, fields: &BTreeMap<Symbol, FieldInfo>) -> u64 {
        let context = TypeContext::item(owner);
        let slots = fields
            .values()
            .map(|info| self.option_value_bytes(self.field_value_bytes(info, context)))
            .fold(0_u64, u64::saturating_add);
        let widest_assignment = fields
            .values()
            .map(|info| self.field_value_bytes(info, context))
            .max()
            .unwrap_or(0);
        slots.saturating_add(widest_assignment)
    }

    fn field_value_bytes(&self, info: &FieldInfo, context: TypeContext) -> u64 {
        self.field_value_bytes_seen(info, context, &mut HashSet::new())
    }

    fn field_value_bytes_seen(
        &self,
        info: &FieldInfo,
        context: TypeContext,
        seen: &mut HashSet<TypeId>,
    ) -> u64 {
        let value = self.type_value_bytes(info.type_id, context, seen);
        if info.optional {
            self.option_value_bytes(value)
        } else {
            value
        }
    }

    fn value_temp_bytes(&self, ty: TypeId, context: TypeContext) -> u64 {
        match self.types.expect_type_shape(ty) {
            TypeShape::Array { element, .. } => VEC_VALUE_BYTES.saturating_add(
                self.type_value_bytes(*element, context.array_element(), &mut HashSet::new()),
            ),
            _ => self.type_value_bytes(ty, context, &mut HashSet::new()),
        }
    }

    fn type_value_bytes(
        &self,
        ty: TypeId,
        context: TypeContext,
        seen: &mut HashSet<TypeId>,
    ) -> u64 {
        if !seen.insert(ty) {
            return WORD_BYTES;
        }

        match self.types.expect_type_shape(ty) {
            TypeShape::Void => 0,
            TypeShape::Node | TypeShape::Custom(_) => NODE_VALUE_BYTES,
            TypeShape::Optional(inner) => {
                let inner = self.type_value_bytes(*inner, context, seen);
                self.option_value_bytes(inner)
            }
            TypeShape::Array { element, .. } => VEC_VALUE_BYTES
                .saturating_add(self.type_value_bytes(*element, context.array_element(), seen)),
            TypeShape::Struct(fields) => fields
                .values()
                .map(|info| {
                    let mut field_seen = seen.clone();
                    self.field_value_bytes_seen(info, context, &mut field_seen)
                })
                .fold(0_u64, u64::saturating_add),
            TypeShape::Enum(variants) => {
                let widest = variants
                    .values()
                    .map(|&payload| {
                        let mut variant_seen = seen.clone();
                        self.type_value_bytes(payload, context, &mut variant_seen)
                    })
                    .max()
                    .unwrap_or(0);
                WORD_BYTES.saturating_add(widest)
            }
            TypeShape::Ref(def_id) => {
                let target = self.types.expect_def_output(*def_id);
                if target == TYPE_VOID {
                    return NODE_VALUE_BYTES;
                }
                if self.model.is_boxed_ref(context, ty) {
                    return WORD_BYTES;
                }
                self.type_value_bytes(target, context, seen)
            }
        }
    }

    fn option_value_bytes(&self, value_bytes: u64) -> u64 {
        align_to_word(value_bytes.saturating_add(OPTION_TAG_BYTES))
    }
}

fn align_to_word(bytes: u64) -> u64 {
    let rem = bytes % WORD_BYTES;
    if rem == 0 {
        return bytes;
    }
    bytes.saturating_add(WORD_BYTES - rem)
}
