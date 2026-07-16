//! Focused capture-type and producer-flow normalization.

use super::planner::CaptureTypePlanner;
use super::*;
use crate::compiler::analyze::types::capture_type::{CaptureTypePlan, CaptureTypePlanKind};
use crate::compiler::ids::TypeDeclId;

// Past this point annotations crowd out the primary cause and the removal hint.
const SUPPRESSED_CAPTURE_ANNOTATION_LIMIT: usize = 8;

#[cfg(test)]
#[path = "normalize_tests.rs"]
mod tests;

impl RawOutputGraph {
    pub(crate) fn normalize(
        self,
        types: &mut crate::compiler::analyze::types::type_analysis::TypeAnalysisBuilder,
        interner: &crate::core::Interner,
        diagnostics: &mut Diagnostics,
    ) {
        NormalizationSession::new(&self, types, interner, diagnostics).run();
    }

    fn blocked_captures(&self, raw_types: &RawTypeSnapshot) -> HashSet<RawCaptureId> {
        let mut blocked = self
            .captures
            .iter()
            .enumerate()
            .filter_map(|(index, capture)| {
                let fact = capture.observation.contract.fact;
                (!fact.is_valid() || raw_types.type_contains_invalid(fact.field().final_type))
                    .then_some(RawCaptureId(index as u32))
            })
            .collect::<HashSet<_>>();

        for alternation in self.alternations.values() {
            let Some(field) = alternation.incompatible_field else {
                continue;
            };
            let Some(output) = alternation.fields.get(&field) else {
                continue;
            };
            blocked.extend(output.producers.iter().copied());
        }
        blocked
    }

    fn omitted_captures(&self) -> HashSet<RawCaptureId> {
        let mut omitted = HashSet::new();
        for alternation in self.alternations.values() {
            for (&name, field) in &alternation.fields {
                if !alternation
                    .alternatives
                    .iter()
                    .any(|alternative| alternative.omissions.contains(&name))
                {
                    continue;
                }
                omitted.extend(field.producers.iter().copied());
            }
        }
        omitted
    }
}

struct NormalizationSession<'a, 'd> {
    graph: &'a RawOutputGraph,
    raw_types: RawTypeSnapshot,
    types: &'d mut crate::compiler::analyze::types::type_analysis::TypeAnalysisBuilder,
    interner: &'a crate::core::Interner,
    diagnostics: &'d mut Diagnostics,
}

impl<'a, 'd> NormalizationSession<'a, 'd> {
    fn new(
        graph: &'a RawOutputGraph,
        types: &'d mut crate::compiler::analyze::types::type_analysis::TypeAnalysisBuilder,
        interner: &'a crate::core::Interner,
        diagnostics: &'d mut Diagnostics,
    ) -> Self {
        Self {
            graph,
            raw_types: RawTypeSnapshot::new(types),
            types,
            interner,
            diagnostics,
        }
    }

    fn run(mut self) {
        let captures = CaptureNormalizer::new(&mut self).run();
        self.types.analysis.field_completions.clear();
        let flow_count = self.graph.flows.len();
        let mut flows = FlowNormalizer::new(&mut self, &captures);
        for index in 0..flow_count {
            flows.normalize(RawFlowId(index as u32));
        }
    }
}

struct CaptureNormalizer<'s, 'a, 'd> {
    session: &'s mut NormalizationSession<'a, 'd>,
    blocked: HashSet<RawCaptureId>,
    omitted: HashSet<RawCaptureId>,
}

enum DeferredCaptureDiagnostic {
    InvalidCaptureType {
        span: Span,
        reason: &'static str,
    },
    SuppressedValue {
        capture_id: RawCaptureId,
        capture_type: BuiltInCaptureType,
        capture_type_span: Span,
        plan: CaptureTypePlan,
    },
}

impl<'s, 'a, 'd> CaptureNormalizer<'s, 'a, 'd> {
    fn new(session: &'s mut NormalizationSession<'a, 'd>) -> Self {
        let blocked = session.graph.blocked_captures(&session.raw_types);
        let omitted = session.graph.omitted_captures();
        Self {
            session,
            blocked,
            omitted,
        }
    }

    fn run(mut self) -> HashMap<RawCaptureId, NormalizedField> {
        let mut normalized = HashMap::new();
        let mut deferred_diagnostics = Vec::new();
        // Suppression warnings need every successful plan to recognize nested
        // boundaries. Invalid-type errors join the queue to preserve capture order.
        for index in 0..self.session.graph.captures.len() {
            let id = RawCaptureId(index as u32);
            let capture = self.session.graph.captures[index].clone();
            let (field, deferred_diagnostic) = self.normalize(id, &capture);
            normalized.insert(id, field);
            if let Some(deferred_diagnostic) = deferred_diagnostic {
                deferred_diagnostics.push(deferred_diagnostic);
            }
        }

        let suppression_boundary_ids = deferred_diagnostics
            .iter()
            .filter_map(|diagnostic| match diagnostic {
                DeferredCaptureDiagnostic::SuppressedValue { capture_id, .. } => Some(*capture_id),
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
                DeferredCaptureDiagnostic::SuppressedValue {
                    capture_id,
                    capture_type,
                    capture_type_span,
                    plan,
                } => {
                    let capture = self.session.graph.capture(capture_id).clone();
                    self.report_suppressed_value(
                        capture_id,
                        &capture,
                        capture_type,
                        capture_type_span,
                        &plan,
                        &suppression_boundary_ids,
                    );
                }
            }
        }
        normalized
    }

    fn normalize(
        &mut self,
        id: RawCaptureId,
        capture: &RawCaptureOutput,
    ) -> (NormalizedField, Option<DeferredCaptureDiagnostic>) {
        let raw_field = capture
            .observation
            .emitted_field
            .unwrap_or(capture.observation.contract.fact.field());
        let ordinary = NormalizedField::ordinary(raw_field, &self.session.raw_types);
        let pattern = Pattern::CapturedPattern(capture.occurrence.clone());

        let RawCaptureIntent::BuiltIn { capture_type, span } = capture.observation.intent else {
            return (ordinary, None);
        };
        if self.blocked.contains(&id) {
            return (ordinary, None);
        }

        let mut planner = CaptureTypePlanner::new(&self.session.raw_types, self.session.types);
        let planned = match planner.plan(
            capture_type,
            capture.observation.contract,
            self.omitted.contains(&id),
        ) {
            Ok(planned) => planned,
            Err(reason) => {
                return (
                    ordinary,
                    Some(DeferredCaptureDiagnostic::InvalidCaptureType { span, reason }),
                );
            }
        };

        let deferred_diagnostic = planned.plan.suppresses_semantic_data().then(|| {
            DeferredCaptureDiagnostic::SuppressedValue {
                capture_id: id,
                capture_type,
                capture_type_span: span,
                plan: planned.plan.clone(),
            }
        });

        self.session.types.analysis.capture_facts.insert(
            pattern,
            CaptureFact::built_in(
                capture.observation.contract.fact.kind(),
                capture_type,
                planned.plan,
            ),
        );
        (planned.field, deferred_diagnostic)
    }

    fn report_suppressed_value(
        &mut self,
        capture_id: RawCaptureId,
        capture: &RawCaptureOutput,
        capture_type: BuiltInCaptureType,
        capture_type_span: Span,
        plan: &CaptureTypePlan,
        suppression_boundary_ids: &HashSet<RawCaptureId>,
    ) {
        assert_eq!(
            capture.source, capture_type_span.source,
            "capture and capture type must belong to the same source"
        );
        let captured_type_id = capture.observation.contract.fact.field().final_type;
        let suppressed_value =
            SuppressedValue::from_plan(&self.session.raw_types, captured_type_id, plan);
        let capture_name = self
            .session
            .interner
            .resolve(capture.observation.name)
            .to_owned();
        let (capture_type_name, replacement_description) = match capture_type {
            BuiltInCaptureType::Text => ("text", "source text"),
            BuiltInCaptureType::Bool => ("bool", "a presence boolean"),
        };
        let value_description = suppressed_value.description();
        let primary_capture_span = capture_span(capture);
        let suppressed_capture_ids = SuppressedCaptureCollector::new(
            self.session.graph,
            &self.session.raw_types,
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
                .or_else(|| inner_pattern_span(&capture.occurrence, capture.source));
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
                let suppressed_capture = self.session.graph.capture(capture_id);
                let annotation = if hidden_capture_count > 0
                    && index + 1 == SUPPRESSED_CAPTURE_ANNOTATION_LIMIT
                {
                    format!(
                        "this captured value is suppressed by `@{capture_name} :: {capture_type_name}`; {} not shown",
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
    graph: &'a RawOutputGraph,
    raw_types: &'a RawTypeSnapshot,
    suppression_boundary_ids: &'a HashSet<RawCaptureId>,
    primary_capture_id: RawCaptureId,
    seen_types: HashSet<TypeId>,
    capture_ids: BTreeSet<RawCaptureId>,
}

impl<'a> SuppressedCaptureCollector<'a> {
    fn new(
        graph: &'a RawOutputGraph,
        raw_types: &'a RawTypeSnapshot,
        suppression_boundary_ids: &'a HashSet<RawCaptureId>,
        primary_capture_id: RawCaptureId,
    ) -> Self {
        Self {
            graph,
            raw_types,
            suppression_boundary_ids,
            primary_capture_id,
            seen_types: HashSet::new(),
            capture_ids: BTreeSet::new(),
        }
    }

    fn collect_from(mut self, type_id: TypeId) -> Vec<RawCaptureId> {
        self.visit_type(type_id);
        let mut capture_ids = self.capture_ids.into_iter().collect::<Vec<_>>();
        capture_ids.sort_by_key(|&capture_id| {
            let span = capture_span(self.graph.capture(capture_id));
            (span.source, span.range.start(), span.range.end())
        });
        capture_ids
    }

    fn visit_type(&mut self, type_id: TypeId) {
        if !self.seen_types.insert(type_id) {
            return;
        }

        match self.raw_types.shape(type_id).clone() {
            TypeShape::Ref(declaration) => {
                self.visit_type(self.raw_types.declaration(declaration));
            }
            TypeShape::Option(inner) | TypeShape::List { element: inner, .. } => {
                self.visit_type(inner);
            }
            TypeShape::Variant(cases) => {
                for payload in cases.into_values() {
                    if let Some(type_id) = payload.type_id() {
                        self.visit_type(type_id);
                    }
                }
            }
            TypeShape::Record(_) => {
                for producer_id in self.graph.capture_producer_ids_for_record_type(type_id) {
                    self.visit_capture(producer_id);
                }
            }
            TypeShape::Node | TypeShape::Text | TypeShape::Bool => {}
        }
    }

    fn visit_capture(&mut self, capture_id: RawCaptureId) {
        if capture_id == self.primary_capture_id {
            return;
        }
        if !self.capture_ids.insert(capture_id) {
            return;
        }
        // The boundary's normalized value is lost here, so annotate it. Its
        // children were already suppressed by its own type and belong to its warning.
        if self.suppression_boundary_ids.contains(&capture_id) {
            return;
        }
        let type_id = self
            .graph
            .capture(capture_id)
            .observation
            .contract
            .fact
            .field()
            .final_type;
        self.visit_type(type_id);
    }
}

enum SuppressedValueKind {
    Record(Vec<Symbol>),
    Variant(Vec<Symbol>),
    List,
}

struct SuppressedValue {
    type_id: TypeId,
    kind: SuppressedValueKind,
    repeated: bool,
}

impl SuppressedValue {
    fn from_plan(raw_types: &RawTypeSnapshot, type_id: TypeId, plan: &CaptureTypePlan) -> Self {
        Self::follow_plan(raw_types, type_id, plan, false)
    }

    fn follow_plan(
        raw_types: &RawTypeSnapshot,
        type_id: TypeId,
        plan: &CaptureTypePlan,
        repeated: bool,
    ) -> Self {
        // Follow the frozen plan alongside the raw type: `text` maps list
        // elements, while `bool` can replace an omitted list as one value.
        let type_id = raw_types.resolve_reference_chain(type_id);
        match plan.kind() {
            CaptureTypePlanKind::Option { inner, .. } => {
                let TypeShape::Option(inner_type) = raw_types.shape(type_id) else {
                    unreachable!("option capture-type plan must follow a raw option")
                };
                Self::follow_plan(raw_types, *inner_type, inner, repeated)
            }
            CaptureTypePlanKind::List { element } => {
                let TypeShape::List { element: inner, .. } = raw_types.shape(type_id) else {
                    unreachable!("list capture-type plan must follow a raw list")
                };
                Self::follow_plan(raw_types, *inner, element, true)
            }
            CaptureTypePlanKind::TextTerminal {
                data: TerminalData::Semantic,
            }
            | CaptureTypePlanKind::BoolTerminal {
                data: TerminalData::Semantic,
            } => {
                let kind = match raw_types.shape(type_id) {
                    TypeShape::Record(fields) => {
                        SuppressedValueKind::Record(fields.keys().copied().collect())
                    }
                    TypeShape::Variant(cases) => {
                        SuppressedValueKind::Variant(cases.keys().copied().collect())
                    }
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

fn capture_span(capture: &RawCaptureOutput) -> Span {
    Span::new(capture.source, capture.occurrence.capture().text_range())
}

fn inner_pattern_span(captured_pattern: &CapturedPattern, source: SourceId) -> Option<Span> {
    captured_pattern
        .inner()
        .map(|inner| Span::new(source, inner.text_range()))
}

fn record_field_loss_annotation(
    subject: &str,
    fields: &[Symbol],
    interner: &crate::core::Interner,
) -> String {
    assert!(!fields.is_empty(), "record values must have named fields");
    let field_count = fields.len();
    let formatted_fields = format_member_names(fields, interner, "and");
    if field_count == 1 {
        return format!("{subject} no longer contains field {formatted_fields}");
    }
    format!("{subject} no longer contains fields {formatted_fields}")
}

fn format_member_names(
    members: &[Symbol],
    interner: &crate::core::Interner,
    conjunction: &str,
) -> String {
    const DISPLAY_LIMIT: usize = 4;

    let shown = members
        .iter()
        .take(DISPLAY_LIMIT)
        .map(|&name| format!("`{}`", interner.resolve(name)))
        .collect::<Vec<_>>();
    let remaining = members.len().saturating_sub(shown.len());
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
pub(super) struct RawTypeSnapshot {
    types: Vec<Option<TypeShape>>,
    declarations: BTreeMap<TypeDeclId, TypeId>,
    invalid_containment: HashSet<TypeId>,
}

impl RawTypeSnapshot {
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
            .expect("raw capture type must be registered")
    }

    pub(super) fn declaration(&self, declaration: TypeDeclId) -> TypeId {
        *self
            .declarations
            .get(&declaration)
            .expect("raw referenced declaration must have a body")
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
                .expect("raw referenced declaration has a body");
            containers
                .get_mut(child.0 as usize)
                .expect("raw child type must be registered")
                .push(container);
            continue;
        }
        for child in shape.child_type_ids() {
            containers
                .get_mut(child.0 as usize)
                .expect("raw child type must be registered")
                .push(container);
        }
    }

    let mut contains_invalid = invalid.clone();
    let mut pending = invalid.iter().copied().collect::<Vec<_>>();
    while let Some(type_id) = pending.pop() {
        for &container in containers
            .get(type_id.0 as usize)
            .expect("invalid raw type must be registered")
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
    fn ordinary(info: RecordField, raw_types: &RawTypeSnapshot) -> Self {
        let on_absence = if matches!(raw_types.shape(info.final_type), TypeShape::List { .. }) {
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

struct FlowNormalizer<'s, 'c, 'a, 'd> {
    session: &'s mut NormalizationSession<'a, 'd>,
    captures: &'c HashMap<RawCaptureId, NormalizedField>,
    normalized: HashMap<RawFlowId, BTreeMap<Symbol, NormalizedField>>,
    visiting: HashSet<RawFlowId>,
}

impl<'s, 'c, 'a, 'd> FlowNormalizer<'s, 'c, 'a, 'd> {
    fn new(
        session: &'s mut NormalizationSession<'a, 'd>,
        captures: &'c HashMap<RawCaptureId, NormalizedField>,
    ) -> Self {
        Self {
            session,
            captures,
            normalized: HashMap::new(),
            visiting: HashSet::new(),
        }
    }

    fn normalize(&mut self, id: RawFlowId) -> Option<&BTreeMap<Symbol, NormalizedField>> {
        if self.normalized.contains_key(&id) {
            return self.normalized.get(&id);
        }
        if !self.visiting.insert(id) {
            unreachable!("pattern output graph follows the finite AST, not definition refs")
        }

        let output = self.session.graph.flow(id).clone();
        let raw_fields = match &output.flow {
            RawPatternFlow::Fields(fields) => fields.clone(),
            RawPatternFlow::NoValue | RawPatternFlow::Value => {
                self.visiting.remove(&id);
                return None;
            }
        };
        let alternation = self
            .session
            .graph
            .alternations
            .get(&output.occurrence)
            .cloned();
        let mut normalized = BTreeMap::new();
        let mut completions = BTreeMap::new();

        for (&name, raw_field) in &raw_fields.fields {
            let mut field = self.normalize_sources(&output, name, raw_field);
            if let Some(alternation) = &alternation {
                let omitted = alternation
                    .alternatives
                    .iter()
                    .any(|alternative| alternative.omissions.contains(&name));
                let completion = if omitted {
                    let (completed, completion) = field.complete_absence(self.session.types);
                    field = completed;
                    completion
                } else {
                    FieldCompletion::AlwaysPresent
                };
                completions.insert(name, completion);
            } else {
                field = self.adapt_to_raw_output(raw_field, field);
            }
            normalized.insert(name, field);
        }

        let fields = normalized
            .iter()
            .map(|(&name, field)| (name, field.info))
            .collect();
        self.session
            .types
            .replace_record_fields(raw_fields.type_id, fields);

        if alternation.is_some() {
            self.session.types.analysis.field_completions.insert(
                output.occurrence.clone(),
                FieldCompletions::new(completions),
            );
        }
        self.normalized.insert(id, normalized);
        self.visiting.remove(&id);
        self.normalized.get(&id)
    }

    fn normalize_sources(
        &mut self,
        owner: &RawPatternOutput,
        name: Symbol,
        field: &RawFieldOutput,
    ) -> NormalizedField {
        let mut sources = field.sources.iter();
        let first = *sources
            .next()
            .expect("raw public field must retain an immediate source");
        let mut normalized = self.normalize_source(first);
        if !self
            .session
            .graph
            .alternations
            .contains_key(&owner.occurrence)
        {
            return normalized;
        }
        for &source in sources {
            let other = self.normalize_source(source);
            match unify_normalized_fields(self.session.types, normalized, other) {
                Ok(unified) => normalized = unified,
                Err(()) => {
                    // Raw inference already owns structural incompatibilities.
                    // A mismatch here can only be introduced by written capture
                    // types, so report it without rewriting the raw ownership
                    // graph that subsequent fields still depend on.
                    self.session
                        .diagnostics
                        .report(
                            DiagnosticKind::IncompatibleCaptureTypes,
                            Span::new(owner.source, owner.occurrence.text_range()),
                        )
                        .detail(self.session.interner.resolve(name))
                        .emit();
                }
            }
        }
        normalized
    }

    fn normalize_source(&mut self, source: RawFieldSource) -> NormalizedField {
        match source {
            RawFieldSource::Capture(capture) => *self
                .captures
                .get(&capture)
                .expect("every raw capture has a normalized field"),
            RawFieldSource::Flow { flow, field } => *self
                .normalize(flow)
                .and_then(|fields| fields.get(&field))
                .expect("field source must survive normalization"),
        }
    }

    fn adapt_to_raw_output(
        &mut self,
        raw_output: &RawFieldOutput,
        mut field: NormalizedField,
    ) -> NormalizedField {
        let source = raw_output
            .sources
            .first()
            .copied()
            .expect("raw field has an immediate source");
        let raw_source = self.raw_source_info(source);
        if raw_source == raw_output.info {
            return field;
        }

        if matches!(
            field.on_absence,
            AbsencePolicy::CompleteWith(FieldCompletion::Absent | FieldCompletion::False)
        ) && matches!(
            self.session.raw_types.shape(raw_output.info.final_type),
            TypeShape::Option(inner) if *inner == raw_source.final_type
        ) {
            return field;
        }

        let Some(final_type) = self.adapt_final_type(
            raw_source.final_type,
            raw_output.info.final_type,
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
        raw_source: TypeId,
        raw_output: TypeId,
        normalized: TypeId,
    ) -> Option<TypeId> {
        if raw_source == raw_output {
            return Some(normalized);
        }

        match self.session.raw_types.shape(raw_output).clone() {
            TypeShape::Option(inner) => {
                let inner = self.adapt_final_type(raw_source, inner, normalized)?;
                Some(self.session.types.intern_option(inner))
            }
            TypeShape::List { element, minimum } => {
                let element = self.adapt_final_type(raw_source, element, normalized)?;
                Some(
                    self.session
                        .types
                        .intern_type(TypeShape::List { element, minimum }),
                )
            }
            _ => None,
        }
    }

    fn raw_source_info(&self, source: RawFieldSource) -> RecordField {
        match source {
            RawFieldSource::Capture(capture) => self
                .session
                .graph
                .capture(capture)
                .observation
                .emitted_field
                .expect("field-producing capture has a raw emitted field"),
            RawFieldSource::Flow { flow, field } => self
                .session
                .graph
                .flow(flow)
                .flow
                .fields()
                .and_then(|fields| fields.get(&field))
                .map(|field| field.info)
                .expect("field source must reference a raw fields flow"),
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
