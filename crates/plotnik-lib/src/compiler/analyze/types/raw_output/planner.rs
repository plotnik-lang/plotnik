//! Capture-type planning against the frozen raw type graph.

use super::normalize::{NormalizedField, OmissionPolicy, RawTypeSnapshot};
use super::*;

pub(super) struct PlannedCapture {
    pub(super) plan: CaptureTypePlan,
    pub(super) field: NormalizedField,
}

pub(super) struct CaptureTypePlanner<'a, 'b> {
    raw: &'a RawTypeSnapshot,
    types: &'b mut crate::compiler::analyze::types::type_analysis::TypeAnalysisBuilder,
}

impl<'a, 'b> CaptureTypePlanner<'a, 'b> {
    pub(super) fn new(
        raw: &'a RawTypeSnapshot,
        types: &'b mut crate::compiler::analyze::types::type_analysis::TypeAnalysisBuilder,
    ) -> Self {
        Self { raw, types }
    }

    pub(super) fn plan(
        &mut self,
        capture_type: BuiltInCaptureType,
        contract: RawCaptureContract,
        observes_omission: bool,
    ) -> Result<PlannedCapture, &'static str> {
        match capture_type {
            BuiltInCaptureType::Str => self.plan_str(contract),
            BuiltInCaptureType::Bool => self.plan_bool(contract.fact.field(), observes_omission),
        }
    }

    fn plan_str(&mut self, contract: RawCaptureContract) -> Result<PlannedCapture, &'static str> {
        let raw = contract.fact.field();
        let (mut plan, mut absorbs_null) = self.str_plan(
            raw.type_id,
            contract.zero_node_terminal,
            &mut HashSet::new(),
        )?;
        if raw.optional {
            let optional = self
                .types
                .intern_type(TypeShape::Optional(plan.final_type()));
            plan = CaptureTypePlan::optional(optional, OptionalCaptureTypeMode::Preserve, plan);
            absorbs_null = true;
        }
        let omission = if absorbs_null {
            OmissionPolicy::Value(FieldFallback::Null)
        } else if matches!(
            self.types.in_progress().type_shape(plan.final_type()),
            Some(TypeShape::Array { .. })
        ) {
            OmissionPolicy::Value(FieldFallback::EmptyArray)
        } else {
            OmissionPolicy::FieldOptional
        };
        Ok(PlannedCapture {
            field: NormalizedField {
                info: FieldInfo::required(plan.final_type()),
                omission,
            },
            plan,
        })
    }

    fn str_plan(
        &mut self,
        type_id: TypeId,
        zero_node_terminal: bool,
        visiting: &mut HashSet<TypeId>,
    ) -> Result<(CaptureTypePlan, bool), &'static str> {
        if !visiting.insert(type_id) {
            return Err("capture type `str` cannot normalize a recursive container type");
        }

        let result = match self.raw.shape(type_id) {
            TypeShape::Node => Ok((
                CaptureTypePlan::str_terminal(TYPE_STR, TerminalData::NodeRepresentation),
                false,
            )),
            TypeShape::Struct(_) | TypeShape::Variant(_) => {
                let final_type = if zero_node_terminal {
                    self.types.intern_type(TypeShape::Optional(TYPE_STR))
                } else {
                    TYPE_STR
                };
                Ok((
                    CaptureTypePlan::str_terminal(final_type, TerminalData::Semantic),
                    zero_node_terminal,
                ))
            }
            TypeShape::Optional(inner) => {
                let (inner, _) = self.str_plan(*inner, false, visiting)?;
                let optional = self
                    .types
                    .intern_type(TypeShape::Optional(inner.final_type()));
                Ok((
                    CaptureTypePlan::optional(optional, OptionalCaptureTypeMode::Preserve, inner),
                    true,
                ))
            }
            TypeShape::Array { element, non_empty } => {
                let (element, _) = self.str_plan(*element, false, visiting)?;
                let array = self.types.intern_type(TypeShape::Array {
                    element: element.final_type(),
                    non_empty: *non_empty,
                });
                Ok((CaptureTypePlan::array(array, element), false))
            }
            TypeShape::Ref(target) => {
                self.str_plan(self.raw.definition(*target), zero_node_terminal, visiting)
            }
            TypeShape::Void => Err("a capture type requires an ordinary captured value"),
            TypeShape::Str | TypeShape::Bool | TypeShape::Custom(_) => {
                unreachable!("a capture type cannot feed another capture type")
            }
        };
        visiting.remove(&type_id);
        result
    }

    fn plan_bool(
        &mut self,
        raw: FieldInfo,
        observes_omission: bool,
    ) -> Result<PlannedCapture, &'static str> {
        let plan = if raw.optional {
            let inner = self.bool_present(raw.type_id, &mut HashSet::new())?;
            CaptureTypePlan::optional(TYPE_BOOL, OptionalCaptureTypeMode::Bool, inner)
        } else {
            self.bool_required(raw.type_id, observes_omission, &mut HashSet::new())?
        };
        Ok(PlannedCapture {
            plan,
            field: NormalizedField {
                info: FieldInfo::required(TYPE_BOOL),
                omission: OmissionPolicy::Value(FieldFallback::False),
            },
        })
    }

    fn bool_required(
        &mut self,
        type_id: TypeId,
        observes_omission: bool,
        visiting: &mut HashSet<TypeId>,
    ) -> Result<CaptureTypePlan, &'static str> {
        if !visiting.insert(type_id) {
            return Err("capture type `bool` cannot normalize a recursive container type");
        }
        let result = match self.raw.shape(type_id) {
            TypeShape::Optional(inner) => {
                let inner = self.bool_present(*inner, visiting)?;
                Ok(CaptureTypePlan::optional(
                    TYPE_BOOL,
                    OptionalCaptureTypeMode::Bool,
                    inner,
                ))
            }
            TypeShape::Ref(target) => {
                self.bool_required(self.raw.definition(*target), observes_omission, visiting)
            }
            TypeShape::Array { .. } if observes_omission => Ok(CaptureTypePlan::bool_terminal(
                TYPE_BOOL,
                TerminalData::Semantic,
            )),
            TypeShape::Array { .. } => Err(
                "capture type `bool` cannot be applied to this list; capture an optional value inside the list, or inspect whether the list is empty after parsing",
            ),
            TypeShape::Node | TypeShape::Struct(_) | TypeShape::Variant(_) if observes_omission => {
                Ok(CaptureTypePlan::bool_terminal(
                    TYPE_BOOL,
                    terminal_data(self.raw.shape(type_id)),
                ))
            }
            TypeShape::Node | TypeShape::Struct(_) | TypeShape::Variant(_) => Err(
                "capture type `bool` requires a value that may be absent; this capture is always present",
            ),
            TypeShape::Void => Err("a capture type requires an ordinary captured value"),
            TypeShape::Str | TypeShape::Bool | TypeShape::Custom(_) => {
                unreachable!("a capture type cannot feed another capture type")
            }
        };
        visiting.remove(&type_id);
        result
    }

    fn bool_present(
        &mut self,
        type_id: TypeId,
        visiting: &mut HashSet<TypeId>,
    ) -> Result<CaptureTypePlan, &'static str> {
        match self.raw.shape(type_id) {
            TypeShape::Optional(_) => self.bool_required(type_id, false, visiting),
            TypeShape::Ref(target) => self.bool_present(self.raw.definition(*target), visiting),
            TypeShape::Node
            | TypeShape::Struct(_)
            | TypeShape::Variant(_)
            | TypeShape::Array { .. } => Ok(CaptureTypePlan::bool_terminal(
                TYPE_BOOL,
                terminal_data(self.raw.shape(type_id)),
            )),
            TypeShape::Void => Err("a capture type requires an ordinary captured value"),
            TypeShape::Str | TypeShape::Bool | TypeShape::Custom(_) => {
                unreachable!("a capture type cannot feed another capture type")
            }
        }
    }
}

fn terminal_data(shape: &TypeShape) -> TerminalData {
    match shape {
        TypeShape::Node => TerminalData::NodeRepresentation,
        _ => TerminalData::Semantic,
    }
}
