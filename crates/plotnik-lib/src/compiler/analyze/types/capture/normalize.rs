//! Focused capture-type and producer-flow normalization.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use super::planner::CaptureTypePlanner;
use super::*;
use crate::compiler::analyze::types::type_description::describe_type;
use crate::compiler::analyze::types::type_shape::{
    CasePayload, ListMinimum, RecordField, TYPE_NODE, TypeId, TypeShape,
};
use crate::compiler::diagnostics::report::{DiagnosticKind, Diagnostics};
use crate::compiler::ids::TypeDeclId;
use crate::compiler::parse::ast::Pattern;

// Past this point annotations crowd out the primary cause and the removal hint.
const SUPPRESSED_CAPTURE_ANNOTATION_LIMIT: usize = 8;
const SUPPRESSED_MEMBER_NAME_LIMIT: usize = 4;

struct NormalizationInput {
    captures: Vec<RecordedCapture>,
    blocked_capture_ids: HashSet<CaptureId>,
    field_flows: HashMap<Pattern, InferredFieldFlow>,
    order: Vec<Pattern>,
    capture_producers_by_record_type: HashMap<TypeId, BTreeSet<CaptureId>>,
}

impl CaptureProvenance {
    pub(crate) fn normalize(
        self,
        pattern_order: Vec<Pattern>,
        types: &mut crate::compiler::analyze::types::type_analysis::TypeAnalysisBuilder,
        interner: &crate::core::Interner,
        diagnostics: &mut Diagnostics,
    ) {
        let mut field_flows = HashMap::new();
        for pattern in &pattern_order {
            let shape = types
                .analysis
                .pattern_result
                .get_mut(pattern)
                .expect("inference order references an inferred pattern");
            if let Some(field_flow) = shape.field_flow.take() {
                field_flows.insert(pattern.clone(), field_flow);
            }
        }

        let mut capture_producers_by_record_type = HashMap::<TypeId, BTreeSet<CaptureId>>::new();
        for field_flow in field_flows.values() {
            let producers = capture_producers_by_record_type
                .entry(field_flow.type_id)
                .or_default();
            for field in field_flow.fields.values() {
                producers.extend(field.producers.iter().copied());
            }
        }

        let input = NormalizationInput {
            captures: self.captures,
            blocked_capture_ids: self.blocked_capture_ids,
            field_flows,
            order: pattern_order,
            capture_producers_by_record_type,
        };
        NormalizationSession::new(&input, types, interner, diagnostics).run();
    }
}

impl NormalizationInput {
    fn capture(&self, id: CaptureId) -> &RecordedCapture {
        self.captures
            .get(id.index())
            .expect("capture id references an inferred capture")
    }

    fn field_flow(&self, pattern: &Pattern) -> Option<&InferredFieldFlow> {
        self.field_flows.get(pattern)
    }

    fn blocked_captures(&self, inferred_types: &InferredTypeSnapshot) -> HashSet<CaptureId> {
        let mut blocked_capture_ids = self.blocked_capture_ids.clone();
        blocked_capture_ids.extend(self.captures.iter().enumerate().filter_map(
            |(index, capture)| {
                let fact = capture.observation.contract.fact;
                (!fact.is_valid() || inferred_types.type_contains_invalid(fact.field().final_type))
                    .then_some(CaptureId::from_index(index))
            },
        ));
        blocked_capture_ids
    }

    fn omitted_capture_ids(&self) -> HashSet<CaptureId> {
        let mut omitted_capture_ids = HashSet::new();
        for field_flow in self.field_flows.values() {
            let Some(alternation_omissions) = &field_flow.alternation_omissions else {
                continue;
            };
            for name in alternation_omissions {
                omitted_capture_ids.extend(
                    field_flow
                        .fields
                        .get(name)
                        .expect("omitted field belongs to its alternation flow")
                        .producers
                        .iter()
                        .copied(),
                );
            }
        }
        omitted_capture_ids
    }

    fn capture_producer_ids_for_record_type(&self, record_type_id: TypeId) -> &BTreeSet<CaptureId> {
        self.capture_producers_by_record_type
            .get(&record_type_id)
            .expect("record type retains its capture producers")
    }
}

struct NormalizationSession<'a, 'd> {
    input: &'a NormalizationInput,
    inferred_types: InferredTypeSnapshot,
    types: &'d mut crate::compiler::analyze::types::type_analysis::TypeAnalysisBuilder,
    interner: &'a crate::core::Interner,
    diagnostics: &'d mut Diagnostics,
}

impl<'a, 'd> NormalizationSession<'a, 'd> {
    fn new(
        input: &'a NormalizationInput,
        types: &'d mut crate::compiler::analyze::types::type_analysis::TypeAnalysisBuilder,
        interner: &'a crate::core::Interner,
        diagnostics: &'d mut Diagnostics,
    ) -> Self {
        Self {
            input,
            inferred_types: InferredTypeSnapshot::new(types),
            types,
            interner,
            diagnostics,
        }
    }

    fn run(mut self) {
        let captures = CaptureNormalizer::new(&mut self).run();
        let order = self.input.order.clone();
        let mut field_flows = FieldFlowNormalizer::new(&mut self, &captures);
        for pattern in order {
            field_flows.normalize(&pattern);
        }
    }
}

struct CaptureNormalizer<'s, 'a, 'd> {
    session: &'s mut NormalizationSession<'a, 'd>,
    blocked_capture_ids: HashSet<CaptureId>,
    omitted_capture_ids: HashSet<CaptureId>,
}

enum DeferredCaptureDiagnostic {
    InvalidCaptureType { span: Span, reason: &'static str },
    SuppressedValue(SuppressedValueDiagnostic),
}

struct SuppressedValueDiagnostic {
    capture_id: CaptureId,
}

impl<'s, 'a, 'd> CaptureNormalizer<'s, 'a, 'd> {
    fn new(session: &'s mut NormalizationSession<'a, 'd>) -> Self {
        let blocked_capture_ids = session.input.blocked_captures(&session.inferred_types);
        let omitted_capture_ids = session.input.omitted_capture_ids();
        Self {
            session,
            blocked_capture_ids,
            omitted_capture_ids,
        }
    }

    fn run(mut self) -> HashMap<CaptureId, NormalizedField> {
        let mut normalized = HashMap::new();
        let mut deferred_diagnostics = Vec::new();
        // Suppression warnings need every successful plan to recognize nested
        // boundaries. Invalid-type errors join the queue to preserve capture order.
        for index in 0..self.session.input.captures.len() {
            let id = CaptureId::from_index(index);
            let (field, deferred_diagnostic) = self.normalize(id);
            normalized.insert(id, field);
            if let Some(deferred_diagnostic) = deferred_diagnostic {
                deferred_diagnostics.push(deferred_diagnostic);
            }
        }

        let suppression_boundary_ids = deferred_diagnostics
            .iter()
            .filter_map(|diagnostic| match diagnostic {
                DeferredCaptureDiagnostic::SuppressedValue(request) => Some(request.capture_id),
                DeferredCaptureDiagnostic::InvalidCaptureType { .. } => None,
            })
            .collect::<HashSet<_>>();
        for diagnostic in deferred_diagnostics {
            match diagnostic {
                DeferredCaptureDiagnostic::InvalidCaptureType { span, reason } => {
                    self.session
                        .diagnostics
                        .report(DiagnosticKind::InvalidCaptureType, span)
                        .detail(reason)
                        .emit();
                }
                DeferredCaptureDiagnostic::SuppressedValue(request) => {
                    self.report_suppressed_value(request, &suppression_boundary_ids);
                }
            }
        }
        normalized
    }

    fn normalize(&mut self, id: CaptureId) -> (NormalizedField, Option<DeferredCaptureDiagnostic>) {
        let (inferred_field, pattern, intent, contract) = {
            let capture = self.session.input.capture(id);
            (
                capture
                    .observation
                    .produced_field
                    .unwrap_or(capture.observation.contract.fact.field()),
                Pattern::CapturedPattern(capture.occurrence.node().clone()),
                capture.observation.intent,
                capture.observation.contract,
            )
        };
        let ordinary = NormalizedField::ordinary(inferred_field, &self.session.inferred_types);

        let CaptureTypeIntent::BuiltIn { capture_type, span } = intent else {
            return (ordinary, None);
        };
        if self.blocked_capture_ids.contains(&id) {
            return (ordinary, None);
        }

        let mut planner = CaptureTypePlanner::new(&self.session.inferred_types, self.session.types);
        let planned = match planner.plan(
            capture_type,
            contract,
            self.omitted_capture_ids.contains(&id),
        ) {
            Ok(planned) => planned,
            Err(reason) => {
                return (
                    ordinary,
                    Some(DeferredCaptureDiagnostic::InvalidCaptureType { span, reason }),
                );
            }
        };

        let deferred_diagnostic = planned.plan.suppresses_semantic_data().then_some(
            DeferredCaptureDiagnostic::SuppressedValue(SuppressedValueDiagnostic {
                capture_id: id,
            }),
        );

        self.session.types.analysis.capture_facts.insert(
            pattern,
            CaptureFact::built_in(contract.fact.kind(), capture_type, planned.plan),
        );
        (planned.field, deferred_diagnostic)
    }

    fn report_suppressed_value(
        &mut self,
        request: SuppressedValueDiagnostic,
        suppression_boundary_ids: &HashSet<CaptureId>,
    ) {
        let capture_id = request.capture_id;
        let (
            captured_type_id,
            capture_name_symbol,
            pattern,
            primary_capture_span,
            inner_pattern_span,
            capture_type,
        ) = {
            let capture = self.session.input.capture(capture_id);
            let CaptureTypeIntent::BuiltIn { capture_type, .. } = capture.observation.intent else {
                unreachable!("suppressed-value diagnostic belongs to a built-in capture type")
            };
            (
                capture.observation.contract.fact.field().final_type,
                capture.observation.name,
                Pattern::CapturedPattern(capture.occurrence.node().clone()),
                capture_span(capture),
                inner_pattern_span(&capture.occurrence),
                capture_type,
            )
        };
        let (fact_capture_type, plan) = self
            .session
            .types
            .analysis
            .expect_capture_fact(&pattern)
            .built_in_plan()
            .expect("suppressed-value diagnostic requires a normalized capture type");
        assert_eq!(
            fact_capture_type, capture_type,
            "inferred and normalized capture types must agree"
        );
        let suppressed_value =
            SuppressedValue::from_plan(&self.session.inferred_types, captured_type_id, plan);
        let capture_name = self
            .session
            .interner
            .resolve(capture_name_symbol)
            .to_owned();
        let (capture_type_name, replacement_description) = match capture_type {
            BuiltInCaptureType::Text => ("text", "source text"),
            BuiltInCaptureType::Bool => ("bool", "a presence boolean"),
        };
        let value_description = suppressed_value.description();
        let suppressed_capture_ids = SuppressedCaptureCollector::new(
            self.session.input,
            &self.session.inferred_types,
            suppression_boundary_ids,
            capture_id,
        )
        .collect_from(captured_type_id);
        let mut diagnostic = self
            .session
            .diagnostics
            .report(
                DiagnosticKind::CaptureTypeReplacesData,
                primary_capture_span,
            )
            .detail(format!(
                "capture type `{}` replaces the {value_description} captured by `@{capture_name}` with {replacement_description}",
                capture_type_name,
            ));

        let result_shape_loss_annotation = suppressed_value.result_shape_loss_annotation(
            self.session.interner,
            &capture_name,
            suppressed_capture_ids.is_empty(),
        );
        if let Some(annotation) = result_shape_loss_annotation {
            let related_span = self
                .session
                .types
                .type_provenance(suppressed_value.type_id)
                .or(inner_pattern_span);
            if let Some(related_span) = related_span {
                diagnostic = diagnostic.related_to(related_span, annotation);
            }
        }

        if !suppressed_capture_ids.is_empty() {
            let hidden_capture_count = suppressed_capture_ids
                .len()
                .saturating_sub(SUPPRESSED_CAPTURE_ANNOTATION_LIMIT);
            for (index, capture_id) in suppressed_capture_ids
                .into_iter()
                .take(SUPPRESSED_CAPTURE_ANNOTATION_LIMIT)
                .enumerate()
            {
                let suppressed_capture = self.session.input.capture(capture_id);
                let annotation = if hidden_capture_count > 0
                    && index + 1 == SUPPRESSED_CAPTURE_ANNOTATION_LIMIT
                {
                    format!(
                        "this captured value is suppressed by `@{capture_name} :: {capture_type_name}`. {} not shown",
                        format_additional_capture_count(hidden_capture_count)
                    )
                } else {
                    format!(
                        "this captured value is suppressed by `@{capture_name} :: {capture_type_name}`"
                    )
                };
                diagnostic = diagnostic.related_to(capture_span(suppressed_capture), annotation);
            }
        }

        diagnostic
            .hint(format!(
                "remove `:: {}` to return the {value_description} instead",
                capture_type_name
            ))
            .emit();
    }
}

fn format_additional_capture_count(count: usize) -> String {
    if count == 1 {
        return "1 more capture".to_string();
    }
    format!("{count} more captures")
}

struct SuppressedCaptureCollector<'a> {
    input: &'a NormalizationInput,
    inferred_types: &'a InferredTypeSnapshot,
    suppression_boundary_ids: &'a HashSet<CaptureId>,
    primary_capture_id: CaptureId,
    seen_types: HashSet<TypeId>,
    capture_ids: BTreeSet<CaptureId>,
}

impl<'a> SuppressedCaptureCollector<'a> {
    fn new(
        input: &'a NormalizationInput,
        inferred_types: &'a InferredTypeSnapshot,
        suppression_boundary_ids: &'a HashSet<CaptureId>,
        primary_capture_id: CaptureId,
    ) -> Self {
        Self {
            input,
            inferred_types,
            suppression_boundary_ids,
            primary_capture_id,
            seen_types: HashSet::new(),
            capture_ids: BTreeSet::new(),
        }
    }

    fn collect_from(mut self, type_id: TypeId) -> Vec<CaptureId> {
        let mut pending = vec![SuppressedCaptureWork::Type(type_id)];
        while let Some(work) = pending.pop() {
            match work {
                SuppressedCaptureWork::Type(type_id) => {
                    if !self.seen_types.insert(type_id) {
                        continue;
                    }
                    match self.inferred_types.shape(type_id) {
                        TypeShape::Ref(declaration) => pending.push(SuppressedCaptureWork::Type(
                            self.inferred_types.declaration(*declaration),
                        )),
                        TypeShape::Option(inner) | TypeShape::List { element: inner, .. } => {
                            pending.push(SuppressedCaptureWork::Type(*inner));
                        }
                        TypeShape::Variant(cases) => pending.extend(
                            cases
                                .values()
                                .copied()
                                .filter_map(CasePayload::type_id)
                                .map(SuppressedCaptureWork::Type),
                        ),
                        TypeShape::Record(_) => pending.extend(
                            self.input
                                .capture_producer_ids_for_record_type(type_id)
                                .iter()
                                .copied()
                                .map(SuppressedCaptureWork::Capture),
                        ),
                        TypeShape::Node | TypeShape::Text | TypeShape::Bool => {}
                    }
                }
                SuppressedCaptureWork::Capture(capture_id) => {
                    if capture_id == self.primary_capture_id || !self.capture_ids.insert(capture_id)
                    {
                        continue;
                    }
                    // The boundary's normalized value is lost here, so annotate it. Its
                    // children were already suppressed by its own type and belong to its warning.
                    if self.suppression_boundary_ids.contains(&capture_id) {
                        continue;
                    }
                    let type_id = self
                        .input
                        .capture(capture_id)
                        .observation
                        .contract
                        .fact
                        .field()
                        .final_type;
                    pending.push(SuppressedCaptureWork::Type(type_id));
                }
            }
        }

        let mut capture_ids = self.capture_ids.into_iter().collect::<Vec<_>>();
        capture_ids.sort_by_key(|&capture_id| {
            let span = capture_span(self.input.capture(capture_id));
            (span.source, span.range.start(), span.range.end())
        });
        capture_ids
    }
}

enum SuppressedCaptureWork {
    Type(TypeId),
    Capture(CaptureId),
}

enum SuppressedValueKind {
    Record(SuppressedMemberSummary),
    Variant(SuppressedMemberSummary),
    List,
}

struct SuppressedMemberSummary {
    displayed_names: Vec<Symbol>,
    total_count: usize,
}

impl SuppressedMemberSummary {
    fn from_names(names: impl ExactSizeIterator<Item = Symbol>) -> Self {
        let total_count = names.len();
        let displayed_names = names.take(SUPPRESSED_MEMBER_NAME_LIMIT).collect();
        Self {
            displayed_names,
            total_count,
        }
    }
}

struct SuppressedValue {
    type_id: TypeId,
    kind: SuppressedValueKind,
    repeated: bool,
}

impl SuppressedValue {
    fn from_plan(
        inferred_types: &InferredTypeSnapshot,
        type_id: TypeId,
        plan: &CaptureTypePlan,
    ) -> Self {
        Self::follow_plan(inferred_types, type_id, plan, false)
    }

    fn follow_plan(
        inferred_types: &InferredTypeSnapshot,
        type_id: TypeId,
        plan: &CaptureTypePlan,
        repeated: bool,
    ) -> Self {
        // Follow the frozen plan alongside the inferred type: `text` maps list
        // elements, while `bool` can replace an omitted list as one value.
        let type_id = inferred_types.resolve_reference_chain(type_id);
        match plan.kind() {
            CaptureTypePlanKind::Option { inner, .. } => {
                let TypeShape::Option(inner_type) = inferred_types.shape(type_id) else {
                    unreachable!("option capture-type plan must follow an inferred option")
                };
                Self::follow_plan(inferred_types, *inner_type, inner, repeated)
            }
            CaptureTypePlanKind::List { element } => {
                let TypeShape::List { element: inner, .. } = inferred_types.shape(type_id) else {
                    unreachable!("list capture-type plan must follow an inferred list")
                };
                Self::follow_plan(inferred_types, *inner, element, true)
            }
            CaptureTypePlanKind::TextTerminal {
                data: TerminalData::Semantic,
            }
            | CaptureTypePlanKind::BoolTerminal {
                data: TerminalData::Semantic,
            } => {
                let kind = match inferred_types.shape(type_id) {
                    TypeShape::Record(fields) => SuppressedValueKind::Record(
                        SuppressedMemberSummary::from_names(fields.keys().copied()),
                    ),
                    TypeShape::Variant(cases) => SuppressedValueKind::Variant(
                        SuppressedMemberSummary::from_names(cases.keys().copied()),
                    ),
                    TypeShape::List { .. } => SuppressedValueKind::List,
                    TypeShape::Node
                    | TypeShape::Text
                    | TypeShape::Bool
                    | TypeShape::Option(_)
                    | TypeShape::Ref(_) => {
                        unreachable!("semantic capture-type terminal must replace a value")
                    }
                };
                Self {
                    type_id,
                    kind,
                    repeated,
                }
            }
            CaptureTypePlanKind::TextTerminal {
                data: TerminalData::NodeRepresentation,
            }
            | CaptureTypePlanKind::BoolTerminal {
                data: TerminalData::NodeRepresentation,
            } => {
                unreachable!("node-representation capture type does not replace semantic data")
            }
        }
    }

    fn description(&self) -> &'static str {
        match (&self.kind, self.repeated) {
            (SuppressedValueKind::Record(_), false) => "record value",
            (SuppressedValueKind::Record(_), true) => "record values",
            (SuppressedValueKind::Variant(_), false) => "variant value",
            (SuppressedValueKind::Variant(_), true) => "variant values",
            (SuppressedValueKind::List, false) => "list value",
            (SuppressedValueKind::List, true) => "list values",
        }
    }

    fn result_shape_loss_annotation(
        &self,
        interner: &crate::core::Interner,
        capture_name: &str,
        include_record_field_annotation: bool,
    ) -> Option<String> {
        let subject = if self.repeated {
            format!("each item in `@{capture_name}`")
        } else {
            format!("`@{capture_name}`")
        };
        // Capture annotations already account for record fields. Variant
        // identity and list contents are separate losses and remain useful.
        match &self.kind {
            SuppressedValueKind::Record(fields) if include_record_field_annotation => {
                Some(record_field_loss_annotation(&subject, fields, interner))
            }
            SuppressedValueKind::Record(_) => None,
            SuppressedValueKind::Variant(cases) => {
                let cases = format_member_names(cases, interner, "or");
                Some(format!(
                    "{subject} no longer identifies the matched case: {cases}"
                ))
            }
            SuppressedValueKind::List => {
                Some(format!("{subject} no longer contains the collected items"))
            }
        }
    }
}

fn capture_span(capture: &RecordedCapture) -> Span {
    let capture_syntax = capture.occurrence.node().capture();
    capture.occurrence.span_of(capture_syntax.text_range())
}

fn inner_pattern_span(captured_pattern: &Located<CapturedPattern>) -> Option<Span> {
    captured_pattern
        .node()
        .inner()
        .map(|inner| captured_pattern.span_of(inner.text_range()))
}

fn record_field_loss_annotation(
    subject: &str,
    fields: &SuppressedMemberSummary,
    interner: &crate::core::Interner,
) -> String {
    assert!(
        fields.total_count > 0,
        "record values must have named fields"
    );
    let formatted_fields = format_member_names(fields, interner, "and");
    if fields.total_count == 1 {
        return format!("{subject} no longer contains field {formatted_fields}");
    }
    format!("{subject} no longer contains fields {formatted_fields}")
}

fn format_member_names(
    members: &SuppressedMemberSummary,
    interner: &crate::core::Interner,
    conjunction: &str,
) -> String {
    let shown = members
        .displayed_names
        .iter()
        .map(|&name| format!("`{}`", interner.resolve(name)))
        .collect::<Vec<_>>();
    let remaining = members.total_count - shown.len();
    if remaining > 0 {
        return format!("{}, {conjunction} {remaining} more", shown.join(", "));
    }
    match shown.as_slice() {
        [] => unreachable!("member list was checked non-empty"),
        [only] => only.clone(),
        [first, second] => format!("{first} {conjunction} {second}"),
        _ => {
            let (last, rest) = shown
                .split_last()
                .expect("non-empty member list has a last element");
            format!("{}, {conjunction} {last}", rest.join(", "))
        }
    }
}

#[derive(Clone)]
pub(super) struct InferredTypeSnapshot {
    types: Vec<Option<TypeShape>>,
    declarations: BTreeMap<TypeDeclId, TypeId>,
    invalid_containment: HashSet<TypeId>,
}

impl InferredTypeSnapshot {
    fn new(types: &crate::compiler::analyze::types::type_analysis::TypeAnalysisBuilder) -> Self {
        let type_shapes = types.type_shapes_snapshot();
        let declarations = type_shapes
            .iter()
            .filter_map(|shape| match shape {
                Some(TypeShape::Ref(declaration)) => Some(*declaration),
                _ => None,
            })
            .collect::<HashSet<_>>()
            .into_iter()
            .map(|declaration| {
                let body = types
                    .in_progress()
                    .declaration_body(declaration)
                    .unwrap_or(TYPE_NODE);
                (declaration, body)
            })
            .collect();
        let invalid_containment =
            compute_invalid_containment(&type_shapes, &declarations, &types.invalid_types);
        Self {
            types: type_shapes,
            declarations,
            invalid_containment,
        }
    }

    pub(super) fn shape(&self, type_id: TypeId) -> &TypeShape {
        self.types
            .get(type_id.0 as usize)
            .and_then(Option::as_ref)
            .expect("inferred capture type must be registered")
    }

    pub(super) fn declaration(&self, declaration: TypeDeclId) -> TypeId {
        *self
            .declarations
            .get(&declaration)
            .expect("inferred referenced declaration must have a body")
    }

    fn resolve_reference_chain(&self, mut type_id: TypeId) -> TypeId {
        let mut seen = HashSet::new();
        while let TypeShape::Ref(declaration) = self.shape(type_id) {
            assert!(
                seen.insert(*declaration),
                "capture-type plan cannot traverse a reference-only cycle"
            );
            type_id = self.declaration(*declaration);
        }
        type_id
    }

    fn type_contains_invalid(&self, type_id: TypeId) -> bool {
        self.invalid_containment.contains(&type_id)
    }
}

fn compute_invalid_containment(
    types: &[Option<TypeShape>],
    declarations: &BTreeMap<TypeDeclId, TypeId>,
    invalid: &HashSet<TypeId>,
) -> HashSet<TypeId> {
    // Work backwards from invalid types. Unlike recursive DFS memoization,
    // reverse reachability remains correct when references form a cycle: each
    // containing type is visited once, after any invalid descendant reaches it.
    let mut containers = vec![Vec::new(); types.len()];
    for (index, shape) in types.iter().enumerate() {
        let Some(shape) = shape else {
            continue;
        };
        let container = TypeId(index as u32);
        if let TypeShape::Ref(declaration) = shape {
            let child = *declarations
                .get(declaration)
                .expect("inferred referenced declaration has a body");
            containers
                .get_mut(child.0 as usize)
                .expect("inferred child type must be registered")
                .push(container);
            continue;
        }
        for child in shape.child_type_ids() {
            containers
                .get_mut(child.0 as usize)
                .expect("inferred child type must be registered")
                .push(container);
        }
    }

    let mut contains_invalid = invalid.clone();
    let mut pending = invalid.iter().copied().collect::<Vec<_>>();
    while let Some(type_id) = pending.pop() {
        for &container in containers
            .get(type_id.0 as usize)
            .expect("invalid inferred type must be registered")
        {
            if contains_invalid.insert(container) {
                pending.push(container);
            }
        }
    }
    contains_invalid
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AbsencePolicy {
    MakeOption,
    CompleteWith(FieldCompletion),
}

#[derive(Clone, Copy, Debug)]
pub(super) struct NormalizedField {
    pub(super) info: RecordField,
    pub(super) on_absence: AbsencePolicy,
}

impl NormalizedField {
    fn ordinary(info: RecordField, inferred_types: &InferredTypeSnapshot) -> Self {
        let on_absence = if matches!(
            inferred_types.shape(info.final_type),
            TypeShape::List { .. }
        ) {
            AbsencePolicy::CompleteWith(FieldCompletion::EmptyList)
        } else {
            AbsencePolicy::MakeOption
        };
        Self { info, on_absence }
    }

    fn complete_absence(
        mut self,
        types: &mut crate::compiler::analyze::types::type_analysis::TypeAnalysisBuilder,
    ) -> (Self, FieldCompletion) {
        let completion = match self.on_absence {
            AbsencePolicy::MakeOption => {
                self.info = RecordField::new(types.intern_option(self.info.final_type));
                FieldCompletion::Absent
            }
            AbsencePolicy::CompleteWith(FieldCompletion::EmptyList) => {
                let TypeShape::List { element, .. } = types
                    .in_progress()
                    .type_shape(self.info.final_type)
                    .cloned()
                    .expect("empty-list completion requires a registered list")
                else {
                    unreachable!("empty-list completion belongs to a list field")
                };
                let list = types.intern_type(TypeShape::List {
                    element,
                    minimum: ListMinimum::Zero,
                });
                self.info = RecordField::new(list);
                FieldCompletion::EmptyList
            }
            AbsencePolicy::CompleteWith(
                completion @ (FieldCompletion::Absent | FieldCompletion::False),
            ) => completion,
            AbsencePolicy::CompleteWith(FieldCompletion::AlwaysPresent) => {
                unreachable!("always-present fields have no absence policy")
            }
        };
        (self, completion)
    }
}

struct FieldFlowNormalizer<'s, 'c, 'a, 'd> {
    session: &'s mut NormalizationSession<'a, 'd>,
    captures: &'c HashMap<CaptureId, NormalizedField>,
    normalized: HashMap<Pattern, BTreeMap<Symbol, NormalizedField>>,
    // Downstream flows can revisit the same producer pair. Report each exact conflict once.
    reported_conflicts: HashSet<(Symbol, Span, Span)>,
}

impl<'s, 'c, 'a, 'd> FieldFlowNormalizer<'s, 'c, 'a, 'd> {
    fn new(
        session: &'s mut NormalizationSession<'a, 'd>,
        captures: &'c HashMap<CaptureId, NormalizedField>,
    ) -> Self {
        Self {
            session,
            captures,
            normalized: HashMap::new(),
            reported_conflicts: HashSet::new(),
        }
    }

    fn normalize(&mut self, pattern: &Pattern) {
        let Some(inferred_flow) = self.session.input.field_flow(pattern).cloned() else {
            return;
        };
        let mut normalized = BTreeMap::new();
        let mut completions = BTreeMap::new();

        for (&name, inferred_field) in &inferred_flow.fields {
            let mut field = self.normalize_sources(
                inferred_flow.alternation_omissions.is_some(),
                name,
                inferred_field,
            );
            if let Some(alternation_omissions) = &inferred_flow.alternation_omissions {
                let conditionally_present = alternation_omissions.contains(&name);
                let completion = if conditionally_present {
                    let (completed, completion) = field.complete_absence(self.session.types);
                    field = completed;
                    completion
                } else {
                    FieldCompletion::AlwaysPresent
                };
                completions.insert(name, completion);
            } else {
                field = self.adapt_to_inferred_output(inferred_field, field);
            }
            normalized.insert(name, field);
        }

        let fields = normalized
            .iter()
            .map(|(&name, field)| (name, field.info))
            .collect();
        self.session
            .types
            .replace_record_fields(inferred_flow.type_id, fields);

        if inferred_flow.alternation_omissions.is_some() {
            self.session
                .types
                .analysis
                .field_completions
                .insert(pattern.clone(), FieldCompletions::new(completions));
        }
        let previous = self.normalized.insert(pattern.clone(), normalized);
        assert!(
            previous.is_none(),
            "inference order contains each field-producing pattern once"
        );
    }

    fn normalize_sources(
        &mut self,
        alternation: bool,
        name: Symbol,
        field: &InferredField,
    ) -> NormalizedField {
        let mut sources = field.sources.iter();
        let first = sources
            .next()
            .expect("inferred public field must retain an immediate source");
        let (mut normalized, mut previous_span) = self.normalize_source_with_span(first);
        let mut previous_type = normalized.info.final_type;
        if !alternation {
            return normalized;
        }
        for source in sources {
            let (other, other_span) = self.normalize_source_with_span(source);
            let right_type = other.info.final_type;
            match unify_normalized_fields(self.session.types, normalized, other) {
                Ok(unified) => {
                    normalized = unified;
                    previous_span = other_span;
                    previous_type = right_type;
                }
                Err(()) => {
                    // Inference already reported the structural conflict that
                    // blocked these producers; normalization must not cascade it.
                    if field.producers.iter().any(|capture_id| {
                        self.session.input.blocked_capture_ids.contains(capture_id)
                    }) {
                        continue;
                    }
                    if !self
                        .reported_conflicts
                        .insert((name, previous_span, other_span))
                    {
                        continue;
                    }
                    // Inference already owns structural incompatibilities. A
                    // mismatch here can only be introduced by written capture
                    // types, so keep its field sources intact for later fields.
                    let types = self.session.types.in_progress();
                    let left_type = describe_type(&types, self.session.interner, previous_type);
                    let right_type = describe_type(&types, self.session.interner, right_type);
                    self.session
                        .diagnostics
                        .report(
                            DiagnosticKind::IncompatibleCaptureTypes,
                            other_span,
                        )
                        .detail(format!(
                            "`@{}` has incompatible types `{left_type}` and `{right_type}` across alternatives after applying capture types",
                            self.session.interner.resolve(name),
                        ))
                        .related_to(
                            previous_span,
                            format!(
                                "`@{}` has type `{left_type}` here",
                                self.session.interner.resolve(name)
                            ),
                        )
                        .hint(
                            "use the same capture type in every alternative, or label the alternatives to produce a variant",
                        )
                        .emit();
                }
            }
        }
        normalized
    }

    fn normalize_source_with_span(&mut self, source: &FieldSource) -> (NormalizedField, Span) {
        let capture_span = source.capture_span();
        (self.normalize_source(source), capture_span)
    }

    fn normalize_source(&mut self, source: &FieldSource) -> NormalizedField {
        match source {
            FieldSource::Capture { capture_id, .. } => *self
                .captures
                .get(capture_id)
                .expect("every inferred capture has a normalized field"),
            FieldSource::Forwarded { pattern, field, .. } => *self
                .normalized
                .get(pattern)
                .and_then(|fields| fields.get(field))
                .expect("inference records a field source before its forwarded field"),
        }
    }

    fn adapt_to_inferred_output(
        &mut self,
        inferred_output: &InferredField,
        mut field: NormalizedField,
    ) -> NormalizedField {
        let source = inferred_output
            .sources
            .first()
            .expect("inferred field has an immediate source");
        let inferred_source = source.info();
        if inferred_source == inferred_output.info {
            return field;
        }

        if matches!(
            field.on_absence,
            AbsencePolicy::CompleteWith(FieldCompletion::Absent | FieldCompletion::False)
        ) && matches!(
            self.session
                .inferred_types
                .shape(inferred_output.info.final_type),
            TypeShape::Option(inner) if *inner == inferred_source.final_type
        ) {
            return field;
        }

        let Some(final_type) = self.adapt_final_type(
            inferred_source.final_type,
            inferred_output.info.final_type,
            field.info.final_type,
        ) else {
            return field;
        };
        field.info = RecordField::new(final_type);
        field.on_absence = if matches!(
            self.session.types.in_progress().type_shape(final_type),
            Some(TypeShape::List { .. })
        ) {
            AbsencePolicy::CompleteWith(FieldCompletion::EmptyList)
        } else {
            AbsencePolicy::MakeOption
        };
        field
    }

    fn adapt_final_type(
        &mut self,
        inferred_source: TypeId,
        inferred_output: TypeId,
        normalized: TypeId,
    ) -> Option<TypeId> {
        if inferred_source == inferred_output {
            return Some(normalized);
        }

        match self.session.inferred_types.shape(inferred_output).clone() {
            TypeShape::Option(inner) => {
                let inner = self.adapt_final_type(inferred_source, inner, normalized)?;
                Some(self.session.types.intern_option(inner))
            }
            TypeShape::List { element, minimum } => {
                let element = self.adapt_final_type(inferred_source, element, normalized)?;
                Some(
                    self.session
                        .types
                        .intern_type(TypeShape::List { element, minimum }),
                )
            }
            _ => None,
        }
    }
}

fn unify_normalized_fields(
    types: &mut crate::compiler::analyze::types::type_analysis::TypeAnalysisBuilder,
    a: NormalizedField,
    b: NormalizedField,
) -> Result<NormalizedField, ()> {
    let final_type = unify_normalized_types(types, a.info.final_type, b.info.final_type)?;
    let on_absence = match (a.on_absence, b.on_absence) {
        (
            AbsencePolicy::CompleteWith(FieldCompletion::False),
            AbsencePolicy::CompleteWith(FieldCompletion::False),
        ) => AbsencePolicy::CompleteWith(FieldCompletion::False),
        (AbsencePolicy::CompleteWith(FieldCompletion::Absent), _)
        | (_, AbsencePolicy::CompleteWith(FieldCompletion::Absent)) => {
            AbsencePolicy::CompleteWith(FieldCompletion::Absent)
        }
        (
            AbsencePolicy::CompleteWith(FieldCompletion::EmptyList),
            AbsencePolicy::CompleteWith(FieldCompletion::EmptyList),
        ) => AbsencePolicy::CompleteWith(FieldCompletion::EmptyList),
        _ => AbsencePolicy::MakeOption,
    };
    Ok(NormalizedField {
        info: RecordField::new(final_type),
        on_absence,
    })
}

fn unify_normalized_types(
    types: &mut crate::compiler::analyze::types::type_analysis::TypeAnalysisBuilder,
    a: TypeId,
    b: TypeId,
) -> Result<TypeId, ()> {
    if types.types_structurally_equal(a, b) {
        return Ok(a);
    }
    let a_shape = types
        .in_progress()
        .type_shape(a)
        .cloned()
        .expect("normalized type must be registered");
    let b_shape = types
        .in_progress()
        .type_shape(b)
        .cloned()
        .expect("normalized type must be registered");
    match (a_shape, b_shape) {
        (TypeShape::Option(a), TypeShape::Option(b)) => {
            let inner = unify_normalized_types(types, a, b)?;
            Ok(types.intern_option(inner))
        }
        (TypeShape::Option(inner), _) => {
            let inner = unify_normalized_types(types, inner, b)?;
            Ok(types.intern_option(inner))
        }
        (_, TypeShape::Option(inner)) => {
            let inner = unify_normalized_types(types, a, inner)?;
            Ok(types.intern_option(inner))
        }
        (
            TypeShape::List {
                element: a,
                minimum: a_minimum,
            },
            TypeShape::List {
                element: b,
                minimum: b_minimum,
            },
        ) => {
            let element = unify_normalized_types(types, a, b)?;
            Ok(types.intern_type(TypeShape::List {
                element,
                minimum: std::cmp::min(a_minimum, b_minimum),
            }))
        }
        _ => Err(()),
    }
}
