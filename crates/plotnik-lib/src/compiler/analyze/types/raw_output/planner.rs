//! Capture-type planning against the frozen raw type graph.

use super::normalize::{AbsencePolicy, NormalizedField, RawTypeSnapshot};
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
        may_be_absent: bool,
    ) -> Result<PlannedCapture, &'static str> {
        match capture_type {
            BuiltInCaptureType::Str => self.plan_str(contract),
            BuiltInCaptureType::Bool => self.plan_bool(contract.fact.field(), may_be_absent),
        }
    }

    fn plan_str(&mut self, contract: RawCaptureContract) -> Result<PlannedCapture, &'static str> {
        let raw = contract.fact.field();
        let (plan, absorbs_null) = self.str_plan(
            raw.final_type,
            contract.zero_node_terminal,
            &mut HashSet::new(),
        )?;
        let on_absence = if absorbs_null {
            AbsencePolicy::CompleteWith(FieldCompletion::Absent)
        } else if matches!(
            self.types.in_progress().type_shape(plan.final_type()),
            Some(TypeShape::List { .. })
        ) {
            AbsencePolicy::CompleteWith(FieldCompletion::EmptyList)
        } else {
            AbsencePolicy::MakeOption
        };
        Ok(PlannedCapture {
            field: NormalizedField {
                info: RecordField::new(plan.final_type()),
                on_absence,
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
                CaptureTypePlan::text_terminal(TYPE_TEXT, TerminalData::NodeRepresentation),
                false,
            )),
            TypeShape::Record(_) | TypeShape::Variant(_) => {
                let final_type = if zero_node_terminal {
                    self.types.intern_option(TYPE_TEXT)
                } else {
                    TYPE_TEXT
                };
                Ok((
                    CaptureTypePlan::text_terminal(final_type, TerminalData::Semantic),
                    zero_node_terminal,
                ))
            }
            TypeShape::Option(inner) => {
                let (inner, _) = self.str_plan(*inner, false, visiting)?;
                let option = self.types.intern_option(inner.final_type());
                Ok((
                    CaptureTypePlan::option(option, OptionMode::Preserve, inner),
                    true,
                ))
            }
            TypeShape::List { element, minimum } => {
                let (element, _) = self.str_plan(*element, false, visiting)?;
                let list = self.types.intern_type(TypeShape::List {
                    element: element.final_type(),
                    minimum: *minimum,
                });
                Ok((CaptureTypePlan::list(list, element), false))
            }
            TypeShape::Ref(target) => {
                self.str_plan(self.raw.definition(*target), zero_node_terminal, visiting)
            }
            TypeShape::NoValue => Err("a capture type requires an ordinary captured value"),
            TypeShape::Text | TypeShape::Bool | TypeShape::Custom(_) => {
                unreachable!("a capture type cannot feed another capture type")
            }
        };
        visiting.remove(&type_id);
        result
    }

    fn plan_bool(
        &mut self,
        raw: RecordField,
        may_be_absent: bool,
    ) -> Result<PlannedCapture, &'static str> {
        let plan = self.bool_required(raw.final_type, may_be_absent, &mut HashSet::new())?;
        Ok(PlannedCapture {
            plan,
            field: NormalizedField {
                info: RecordField::new(TYPE_BOOL),
                on_absence: AbsencePolicy::CompleteWith(FieldCompletion::False),
            },
        })
    }

    fn bool_required(
        &mut self,
        type_id: TypeId,
        may_be_absent: bool,
        visiting: &mut HashSet<TypeId>,
    ) -> Result<CaptureTypePlan, &'static str> {
        if !visiting.insert(type_id) {
            return Err("capture type `bool` cannot normalize a recursive container type");
        }
        let result = match self.raw.shape(type_id) {
            TypeShape::Option(inner) => {
                let inner = self.bool_present(*inner, visiting)?;
                Ok(CaptureTypePlan::option(TYPE_BOOL, OptionMode::Bool, inner))
            }
            TypeShape::Ref(target) => {
                self.bool_required(self.raw.definition(*target), may_be_absent, visiting)
            }
            TypeShape::List { .. } if may_be_absent => Ok(CaptureTypePlan::bool_terminal(
                TYPE_BOOL,
                TerminalData::Semantic,
            )),
            TypeShape::List { .. } => Err(
                "capture type `bool` cannot be applied to this list; capture an optional value inside the list, or inspect whether the list is empty after parsing",
            ),
            TypeShape::Node | TypeShape::Record(_) | TypeShape::Variant(_) if may_be_absent => Ok(
                CaptureTypePlan::bool_terminal(TYPE_BOOL, terminal_data(self.raw.shape(type_id))),
            ),
            TypeShape::Node | TypeShape::Record(_) | TypeShape::Variant(_) => Err(
                "capture type `bool` requires a value that may be absent; this capture is always present",
            ),
            TypeShape::NoValue => Err("a capture type requires an ordinary captured value"),
            TypeShape::Text | TypeShape::Bool | TypeShape::Custom(_) => {
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
            TypeShape::Option(_) => self.bool_required(type_id, false, visiting),
            TypeShape::Ref(target) => self.bool_present(self.raw.definition(*target), visiting),
            TypeShape::Node
            | TypeShape::Record(_)
            | TypeShape::Variant(_)
            | TypeShape::List { .. } => Ok(CaptureTypePlan::bool_terminal(
                TYPE_BOOL,
                terminal_data(self.raw.shape(type_id)),
            )),
            TypeShape::NoValue => Err("a capture type requires an ordinary captured value"),
            TypeShape::Text | TypeShape::Bool | TypeShape::Custom(_) => {
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
