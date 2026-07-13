//! Bottom-up type inference visitor.
//!
//! Traverses the AST and computes `PatternShape` (`RootExtent` plus
//! `PatternFlow`) for each pattern.
//!
//! # Output model
//!
//! Output exists exactly where output syntax is written: `@capture` makes a
//! field, an alternative label makes a variant case, `:: Name` names a type, and a
//! definition name names the definition's result type. Everything else is
//! structural: it matches, and produces nothing.
//!
//! Two consequences shape this module:
//!
//! - **References are opaque.** A definition has one context-free result type.
//!   `(Foo) @val` stores that result in `val`; a bare `(Foo)` matches
//!   structurally and contributes no output. Fields never bubble through a
//!   reference boundary, recursive or not.
//! - **Labeled alternations produce variants in value contexts.** An alternation
//!   `[A: … B: …]` produces a variant type when captured, collected by a
//!   quantifier, or used as a definition body. In a fields context, labels have
//!   no output effect: captures from the alternatives merge into the enclosing
//!   result, and a warning points at the ineffective labels.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::core::{Interner, Symbol};
use rowan::TextRange;

use super::unify::unify_flows;
use crate::compiler::analyze::types::capture_kind::CaptureKind;
use crate::compiler::analyze::types::raw_output::{
    RawCaptureContract, RawCaptureIntent, RawCaptureObservation, RawDefinitionValueRole,
};
use crate::compiler::analyze::types::type_analysis::{
    CustomCaptureTypeOccurrence, TypeAnalysisBuilder,
};
use crate::compiler::analyze::types::type_shape::{
    FieldInfo, PatternFlow, PatternShape, QuantifierKind, TYPE_NODE, TYPE_VOID, TypeId, TypeShape,
};
use crate::compiler::analyze::types::{
    BuiltInCaptureType, CaptureFact, FieldCompletion, FieldCompletions, RawCaptureFact, RootExtent,
};

use crate::compiler::analyze::Located;
use crate::compiler::analyze::names::SymbolTable;
use crate::compiler::analyze::nullability::compute_nullable_defs;
use crate::compiler::analyze::refs::DependencyAnalysis;
use crate::compiler::diagnostics::report::{DiagnosticBuilder, DiagnosticKind, Diagnostics};
use crate::compiler::diagnostics::source::SourceId;
use crate::compiler::diagnostics::span::Span;
use crate::compiler::ids::DefId;
use crate::compiler::parse::ast::{
    AlternationPattern, Alternative, CapturedPattern, DefRef, FieldPattern, Labeling, NodePattern,
    Pattern, QuantifiedPattern, SeqPattern, TokenPattern, is_empty_group,
};

mod diagnostics;
mod flow;
mod recursive_captures;

/// Shared state for a single inference pass over the AST.
pub struct InferState<'a, 'd> {
    pub type_ctx: &'a mut TypeAnalysisBuilder,
    pub interner: &'a mut Interner,
    pub symbol_table: &'a SymbolTable,
    pub dependency_analysis: &'a DependencyAnalysis,
    /// Definitions whose body can match zero nodes (shared with lowering).
    pub nullable_defs: &'a HashSet<DefId>,
    pub(crate) diag: &'d mut Diagnostics,
}

/// Inference visitor for a single pass over the AST.
pub struct InferVisitor<'a, 'd> {
    ctx: InferState<'a, 'd>,
    source: SourceId,
}

/// Whether a quantifier contributes fields, materializes a captured value, is
/// explicitly discarded, or supplies a definition's value. Captured `*`/`+`
/// quantifiers establish a list element boundary; captured `?` establishes an
/// optional value boundary. A bare quantifier is structural. Under `@_`, output
/// is discarded, so no collection boundary is required. At a definition root,
/// the quantifier's list or option is the definition value.
#[derive(Clone, Copy, PartialEq, Eq)]
enum QuantifiedContext {
    Bare,
    Captured,
    Discard,
    DefinitionValue,
}

struct CaptureInner {
    info: PatternShape,
    makes_field_optional: bool,
}

/// Where one capture field lands after the inner pattern has been inferred.
/// A node capture can bubble beside child fields; every other capture owns a
/// fresh one-field scope. Resolving this before capture-type normalization is
/// important: a duplicate bubbling name makes the raw capture invalid, so a
/// built-in capture type must not add a cascading diagnostic.
enum CaptureFieldDestination {
    OwnScope,
    Bubbling {
        fields: BTreeMap<Symbol, FieldInfo>,
        admits_capture: bool,
    },
}

impl CaptureFieldDestination {
    fn admits_capture(&self) -> bool {
        match self {
            Self::OwnScope => true,
            Self::Bubbling { admits_capture, .. } => *admits_capture,
        }
    }

    fn finish(
        self,
        types: &mut TypeAnalysisBuilder,
        capture_name: Symbol,
        field: FieldInfo,
    ) -> PatternFlow {
        match self {
            Self::OwnScope => PatternFlow::Fields(types.intern_single_field(capture_name, field)),
            Self::Bubbling {
                mut fields,
                admits_capture,
            } => {
                if admits_capture {
                    let previous = fields.insert(capture_name, field);
                    assert!(
                        previous.is_none(),
                        "capture destination was validated vacant"
                    );
                }
                PatternFlow::Fields(types.intern_struct(fields))
            }
        }
    }
}

struct RawCaptureValue {
    mechanism: CaptureKind,
    field: FieldInfo,
    zero_node_terminal: bool,
}

impl RawCaptureValue {
    fn node() -> Self {
        Self {
            mechanism: CaptureKind::Node,
            field: FieldInfo::required(TYPE_NODE),
            zero_node_terminal: false,
        }
    }

    fn inferred(mechanism: CaptureKind, field: FieldInfo, zero_node_terminal: bool) -> Self {
        Self {
            mechanism,
            field,
            zero_node_terminal,
        }
    }
}

struct RawCapture {
    occurrence: CapturedPattern,
    name: Symbol,
    value: RawCaptureValue,
    valid: bool,
}

impl RawCapture {
    fn admitted(occurrence: &CapturedPattern, name: Symbol, value: RawCaptureValue) -> Self {
        Self {
            occurrence: occurrence.clone(),
            name,
            value,
            valid: true,
        }
    }

    fn after_validation(
        occurrence: &CapturedPattern,
        name: Symbol,
        value: RawCaptureValue,
        valid: bool,
    ) -> Self {
        Self {
            occurrence: occurrence.clone(),
            name,
            value,
            valid,
        }
    }

    fn fact(&self) -> RawCaptureFact {
        if self.valid {
            return RawCaptureFact::admitted(self.value.mechanism, self.value.field);
        }
        RawCaptureFact::rejected(self.value.mechanism, self.value.field)
    }

    fn observation(&self, intent: RawCaptureIntent) -> RawCaptureObservation {
        let contract = RawCaptureContract::new(self.fact(), self.value.zero_node_terminal);
        RawCaptureObservation::new(self.name, contract, intent)
    }
}

enum ResolvedCaptureType {
    BuiltIn(BuiltInCaptureType, TextRange),
    Custom(Symbol, TextRange),
    Invalid,
    None,
}

impl ResolvedCaptureType {
    fn raw_intent(&self, source: SourceId) -> RawCaptureIntent {
        match self {
            Self::BuiltIn(capture_type, range) => RawCaptureIntent::BuiltIn {
                capture_type: *capture_type,
                span: Span::new(source, *range),
            },
            Self::Custom(name, _) => RawCaptureIntent::Custom(*name),
            Self::Invalid => RawCaptureIntent::Invalid,
            Self::None => RawCaptureIntent::None,
        }
    }
}

fn suggested_builtin_capture_type(name: &str) -> Option<&'static str> {
    match name {
        "string" => Some("str"),
        "boolean" => Some("bool"),
        _ => None,
    }
}

/// A case's payload comes from the alternative body's bubbling captures. A bare
/// reference contributes no payload, so the case carries the tag alone.
/// `[Fn: (FnDef)]` tags which alternative matched; `[Fn: (FnDef) @fn]` also
/// carries the data.
fn case_payload_type(flow: &PatternFlow) -> TypeId {
    match flow {
        PatternFlow::Void | PatternFlow::Value(_) => TYPE_VOID,
        PatternFlow::Fields(t) => *t,
    }
}

impl<'a, 'd> InferVisitor<'a, 'd> {
    pub fn new(ctx: InferState<'a, 'd>, source: SourceId) -> Self {
        Self { ctx, source }
    }

    fn report(&mut self, kind: DiagnosticKind, range: TextRange) -> DiagnosticBuilder<'_> {
        self.ctx.diag.report(kind, Span::new(self.source, range))
    }

    /// Infer the PatternShape for an expression, caching the result.
    ///
    /// The walk only ever descends through one definition's body (a finite AST
    /// tree); references resolve to precomputed results rather than re-entering.
    pub fn infer_pattern(&mut self, pattern: &Located<Pattern>) -> PatternShape {
        if let Some(info) = self
            .ctx
            .type_ctx
            .in_progress()
            .pattern_result(pattern.node())
        {
            return info.clone();
        }

        let info = self.compute_pattern(pattern);
        self.record_result(pattern, info)
    }

    /// Infer a pattern whose value is materialized by a capture or definition.
    /// The value context threads through grammar-field constraints, which
    /// navigate to the value without creating a result scope.
    pub fn infer_pattern_value(&mut self, pattern: &Located<Pattern>) -> PatternShape {
        if let Some(info) = self
            .ctx
            .type_ctx
            .in_progress()
            .pattern_result(pattern.node())
        {
            return info.clone();
        }

        let info = match pattern.node() {
            Pattern::Alternation(alternation) if alternation.labeling() == Labeling::Labeled => {
                self.infer_labeled_alternation(
                    &pattern.wrap(alternation.clone()),
                    OutputContext::Value,
                )
            }
            Pattern::FieldPattern(f) => {
                self.infer_field_pattern_in(&pattern.wrap(f.clone()), OutputContext::Value)
            }
            Pattern::QuantifiedPattern(q) => {
                return self.infer_quantified_pattern_in(
                    &pattern.wrap(q.clone()),
                    QuantifiedContext::DefinitionValue,
                );
            }
            _ => return self.infer_pattern(pattern),
        };
        self.record_result(pattern, info)
    }

    /// Infer a pattern under an explicit discard. Labels remain valid syntax,
    /// but no warning is needed because the user deliberately suppressed the
    /// result.
    fn infer_pattern_discarded(&mut self, pattern: &Located<Pattern>) -> PatternShape {
        if let Some(info) = self
            .ctx
            .type_ctx
            .in_progress()
            .pattern_result(pattern.node())
        {
            return info.clone();
        }

        let info = match pattern.node() {
            Pattern::Alternation(alternation) if alternation.labeling() == Labeling::Labeled => {
                self.infer_labeled_alternation(
                    &pattern.wrap(alternation.clone()),
                    OutputContext::Discard,
                )
            }
            Pattern::FieldPattern(field) => {
                self.infer_field_pattern_in(&pattern.wrap(field.clone()), OutputContext::Discard)
            }
            Pattern::QuantifiedPattern(quantifier) => {
                return self.infer_quantified_pattern_in(
                    &pattern.wrap(quantifier.clone()),
                    QuantifiedContext::Discard,
                );
            }
            _ => return self.infer_pattern(pattern),
        };
        self.record_result(pattern, info)
    }

    fn record_result(&mut self, pattern: &Located<Pattern>, info: PatternShape) -> PatternShape {
        // Composite flow types get their creation site recorded for the naming
        // pass. Children record before parents (`or_insert` keeps the first),
        // so the deepest — most precise — span wins.
        if let Some(type_id) = info.flow.type_id()
            && matches!(
                self.ctx.type_ctx.in_progress().type_shape(type_id),
                Some(TypeShape::Struct(_) | TypeShape::Variant(_))
            )
        {
            let span = Span::new(self.source, pattern.node().text_range());
            self.ctx.type_ctx.record_type_provenance(type_id, span);
        }

        self.ctx
            .type_ctx
            .record_pattern_result(pattern.node().clone(), self.source, info.clone());
        info
    }

    fn compute_pattern(&mut self, pattern: &Located<Pattern>) -> PatternShape {
        match pattern.node() {
            Pattern::NodePattern(n) => self.infer_named_node(&pattern.wrap(n.clone())),
            Pattern::TokenPattern(n) => self.infer_anonymous_node(n),
            Pattern::DefRef(r) => self.infer_ref(r),
            Pattern::SeqPattern(s) => self.infer_seq_pattern(&pattern.wrap(s.clone())),
            Pattern::Alternation(alternation) => match alternation.labeling() {
                Labeling::Labeled => self.infer_labeled_alternation(
                    &pattern.wrap(alternation.clone()),
                    OutputContext::Fields,
                ),
                Labeling::Unlabeled | Labeling::Mixed => {
                    self.infer_unlabeled_alternation(&pattern.wrap(alternation.clone()))
                }
            },
            Pattern::CapturedPattern(c) => self.infer_captured_pattern(&pattern.wrap(c.clone())),
            Pattern::QuantifiedPattern(q) => {
                self.infer_quantified_pattern_in(&pattern.wrap(q.clone()), QuantifiedContext::Bare)
            }
            Pattern::FieldPattern(f) => {
                self.infer_field_pattern_in(&pattern.wrap(f.clone()), OutputContext::Fields)
            }
        }
    }

    /// Named node: matches one position, bubbles up child captures.
    fn infer_named_node(&mut self, node: &Located<NodePattern>) -> PatternShape {
        let children = node.node().children().map(|child| node.wrap(child));
        let merged = self.collect_child_fields(children);
        PatternShape::new(RootExtent::SingleNode, self.merged_fields_flow(merged))
    }

    /// Anonymous node (literal or wildcard): matches one position, produces nothing.
    fn infer_anonymous_node(&mut self, _node: &TokenPattern) -> PatternShape {
        PatternShape::new(RootExtent::SingleNode, PatternFlow::Void)
    }

    /// Reference: an opaque boundary producing the definition's result value.
    ///
    /// The definition's fields never bubble here. A capture may materialize the
    /// value, while a bare reference contributes no output in a fields context.
    /// Non-recursive targets are already inferred (reverse-topological SCC
    /// order), so their concrete output type stands in directly; a recursive
    /// target's output is not known mid-SCC, so it is referenced as
    /// `TypeShape::Ref` and resolved at emission.
    fn infer_ref(&mut self, r: &DefRef) -> PatternShape {
        let Some(name_tok) = r.name() else {
            return PatternShape::void();
        };
        let name = name_tok.text();
        let name_sym = self.ctx.interner.intern(name);

        // No definition: an undefined reference, already diagnosed upstream
        // (`UndefinedReference`). Outside the trust boundary — answer with void.
        let Some(_body) = self.ctx.symbol_table.body(name) else {
            return PatternShape::void();
        };

        // Every symbol-table definition is assigned a DefId during dependency
        // analysis (each appears in exactly one SCC), so a defined ref always
        // resolves — a miss is our bug.
        let def_id = self
            .ctx
            .dependency_analysis
            .def_id_for_sym(name_sym)
            .expect("a defined reference has a DefId");

        // Root extent is precomputed to a fixpoint before inference, so every
        // reference — recursive ones included — delegates its target's extent
        // and the single-node checks stay sound through recursion.
        let root_extent = self
            .ctx
            .type_ctx
            .def_root_extent(def_id)
            .expect("definition root extents are precomputed before inference");

        if self.ctx.dependency_analysis.is_recursive_def(def_id) {
            // A recursive target's output type stays behind `TypeShape::Ref`
            // (resolved at emission). Its void-ness, however, is real as soon
            // as the def is registered: a completed void target must flow
            // Void so the single-referent check sees it. A same-SCC target
            // not yet registered is a pending value here; those capture
            // sites are re-checked once the SCC completes.
            let resolved_output = self.ctx.type_ctx.in_progress().def_output(def_id);
            let flow = match resolved_output {
                Some(output) if output == TYPE_VOID => PatternFlow::Void,
                _ => {
                    let ref_type = self.ctx.type_ctx.intern_type(TypeShape::Ref(def_id));
                    PatternFlow::Value(ref_type)
                }
            };
            return PatternShape::new(root_extent, flow);
        }

        let output =
            self.ctx.type_ctx.in_progress().def_output(def_id).expect(
                "non-recursive reference target is inferred before the referrer (SCC order)",
            );
        let flow = if output == TYPE_VOID {
            PatternFlow::Void
        } else {
            PatternFlow::Value(output)
        };
        PatternShape::new(root_extent, flow)
    }

    /// Sequence: root-extent aggregation and strict field merging.
    fn infer_seq_pattern(&mut self, seq: &Located<SeqPattern>) -> PatternShape {
        let children: Vec<Located<Pattern>> = seq.node().children().map(|c| seq.wrap(c)).collect();

        let root_extent = self.sequence_root_extent(&children);
        let merged = self.collect_child_fields(children.iter().cloned());

        PatternShape::new(root_extent, self.merged_fields_flow(merged))
    }

    /// Merge the bubbling fields of a scope's children. A `Value` child does not
    /// bubble in a fields context: an uncaptured bare reference contributes no
    /// output.
    fn collect_child_fields(
        &mut self,
        children: impl IntoIterator<Item = Located<Pattern>>,
    ) -> BTreeMap<Symbol, FieldInfo> {
        let mut merged_fields: BTreeMap<Symbol, FieldInfo> = BTreeMap::new();

        for child in children {
            let child_info = self.infer_pattern(&child);
            if let PatternFlow::Fields(type_id) = &child_info.flow {
                let fields = self
                    .ctx
                    .type_ctx
                    .in_progress()
                    .expect_struct_fields(*type_id)
                    .clone();
                self.merge_scope_fields(&mut merged_fields, &fields, child.node().text_range());
            }
        }

        merged_fields
    }

    fn merged_fields_flow(&mut self, merged: BTreeMap<Symbol, FieldInfo>) -> PatternFlow {
        if merged.is_empty() {
            return PatternFlow::Void;
        }
        PatternFlow::Fields(self.ctx.type_ctx.intern_struct(merged))
    }

    fn sequence_root_extent(&mut self, children: &[Located<Pattern>]) -> RootExtent {
        match children {
            [] => RootExtent::SingleNode,
            [child] => self.infer_pattern(child).root_extent,
            _ => RootExtent::Other,
        }
    }

    fn infer_labeled_alternation(
        &mut self,
        alternation: &Located<AlternationPattern>,
        output: OutputContext,
    ) -> PatternShape {
        match output {
            OutputContext::Value => self.infer_labeled_alternation_value(alternation),
            OutputContext::Fields => self.infer_labeled_alternation_fields(alternation),
            OutputContext::Discard => self.infer_labeled_alternation_discarded(alternation),
        }
    }

    fn infer_labeled_alternation_value(
        &mut self,
        alternation: &Located<AlternationPattern>,
    ) -> PatternShape {
        let mut cases: BTreeMap<Symbol, TypeId> = BTreeMap::new();
        let mut combined_extent = RootExtent::SingleNode;

        for alternative in alternation.node().alternatives() {
            let label = alternative
                .label()
                .expect("labeled alternative must have a label");
            let label_sym = self.ctx.interner.intern(label.text());

            // A BTreeMap would silently collapse duplicate labels, leaving the variant type
            // with fewer cases than the emitter expects. Reject them instead.
            if cases.contains_key(&label_sym) {
                self.report_duplicate_case_label(label.text_range(), label.text());
                if let Some(body_info) = self.infer_alternative_body(alternation, &alternative) {
                    combined_extent = combined_extent.combine(body_info.root_extent);
                }
                continue;
            }

            let Some(body_info) = self.infer_alternative_body(alternation, &alternative) else {
                // Tag-only case -> Void (no payload).
                cases.insert(label_sym, TYPE_VOID);
                continue;
            };

            combined_extent = combined_extent.combine(body_info.root_extent);
            cases.insert(label_sym, case_payload_type(&body_info.flow));
        }

        let variant_type = self.ctx.type_ctx.intern_type(TypeShape::Variant(cases));
        PatternShape::new(combined_extent, PatternFlow::Value(variant_type))
    }

    fn infer_labeled_alternation_discarded(
        &mut self,
        alternation: &Located<AlternationPattern>,
    ) -> PatternShape {
        self.check_duplicate_labels(alternation);

        let mut combined_extent = RootExtent::SingleNode;
        for alternative in alternation.node().alternatives() {
            let Some(body) = alternative.body() else {
                continue;
            };
            let body_info = self.infer_pattern_discarded(&alternation.wrap(body));
            combined_extent = combined_extent.combine(body_info.root_extent);
        }

        PatternShape::new(combined_extent, PatternFlow::Void)
    }

    /// In a fields context, labels have no output effect. Warn, then merge the
    /// captures produced by each alternative into the enclosing result.
    fn infer_labeled_alternation_fields(
        &mut self,
        alternation: &Located<AlternationPattern>,
    ) -> PatternShape {
        self.check_duplicate_labels(alternation);
        self.report_unused_alternative_labels(alternation.node());

        let mut flows: Vec<PatternFlow> = Vec::new();
        let mut combined_extent = RootExtent::SingleNode;

        for alternative in alternation.node().alternatives() {
            if let Some(body_info) = self.infer_alternative_body(alternation, &alternative) {
                combined_extent = combined_extent.combine(body_info.root_extent);
                flows.push(body_info.flow);
            }
        }

        let pattern = Pattern::Alternation(alternation.node().clone());
        let unified_flow = self.unify_alternation(pattern, flows);

        PatternShape::new(combined_extent, unified_flow)
    }

    fn check_duplicate_labels(&mut self, alternation: &Located<AlternationPattern>) {
        let mut seen: BTreeMap<Symbol, ()> = BTreeMap::new();
        for alternative in alternation.node().alternatives() {
            let Some(label) = alternative.label() else {
                continue;
            };
            let label_sym = self.ctx.interner.intern(label.text());
            if seen.insert(label_sym, ()).is_some() {
                self.report_duplicate_case_label(label.text_range(), label.text());
            }
        }
    }

    fn report_duplicate_case_label(&mut self, range: TextRange, label: &str) {
        self.report(DiagnosticKind::DuplicateAlternationLabel, range)
            .detail(label)
            .emit();
    }

    fn infer_alternative_body(
        &mut self,
        alternation: &Located<AlternationPattern>,
        alternative: &Alternative,
    ) -> Option<PatternShape> {
        alternative
            .body()
            .map(|body| self.infer_pattern(&alternation.wrap(body)))
    }

    fn infer_unlabeled_alternation(
        &mut self,
        alternation: &Located<AlternationPattern>,
    ) -> PatternShape {
        let mut flows: Vec<PatternFlow> = Vec::new();
        let mut combined_extent = RootExtent::SingleNode;

        for alternative in alternation.node().alternatives() {
            if let Some(body) = alternative.body() {
                let info = self.infer_pattern(&alternation.wrap(body));
                combined_extent = combined_extent.combine(info.root_extent);
                flows.push(info.flow);
            }
        }

        for pattern in alternation.node().patterns() {
            let info = self.infer_pattern(&alternation.wrap(pattern));
            combined_extent = combined_extent.combine(info.root_extent);
            flows.push(info.flow);
        }

        let pattern = Pattern::Alternation(alternation.node().clone());
        let unified_flow = self.unify_alternation(pattern, flows);

        PatternShape::new(combined_extent, unified_flow)
    }

    fn unify_alternation(&mut self, pattern: Pattern, flows: Vec<PatternFlow>) -> PatternFlow {
        match unify_flows(self.ctx.type_ctx, flows) {
            Ok(flow) => flow,
            Err(error) => {
                self.ctx
                    .type_ctx
                    .record_alternation_incompatibility(pattern.clone(), error.field());
                self.report_alternative_unify_error(pattern.syntax(), &error);
                PatternFlow::Void
            }
        }
    }

    /// Captured expression: wraps inner's flow into a field.
    ///
    /// Scope creation rules:
    /// - Sequences `{...} @x` and alternations `[...] @x` create new scopes.
    ///   Inner fields become the captured type's fields.
    /// - Other expressions (named nodes, refs) don't create scopes.
    ///   Inner fields bubble up alongside the capture field.
    fn infer_captured_pattern(&mut self, cap: &Located<CapturedPattern>) -> PatternShape {
        let node = cap.node();

        // Suppressive captures don't contribute to the output type. The inner
        // is still inferred for structural validation, but the explicit discard
        // needs neither an ineffective-label warning nor a collection boundary.
        if node.is_suppressive() {
            let info = match node.inner() {
                None => return PatternShape::void(),
                Some(Pattern::QuantifiedPattern(q)) => {
                    self.infer_quantified_pattern_in(&cap.wrap(q), QuantifiedContext::Discard)
                }
                Some(i) => self.infer_pattern_discarded(&cap.wrap(i)),
            };
            return PatternShape::new(info.root_extent, PatternFlow::Void);
        }

        let Some(name_tok) = node.name() else {
            // Recover gracefully
            return node
                .inner()
                .map(|i| self.infer_pattern(&cap.wrap(i)))
                .unwrap_or_else(PatternShape::void);
        };
        let capture_name = self.ctx.interner.intern(&name_tok.text()[1..]); // Strip @ prefix

        let capture_type = self.resolve_capture_type(node);
        let errors_before_raw_capture = self.ctx.diag.error_count();

        let Some(inner) = node.inner() else {
            // A bare capture binds the current node.
            let raw = RawCapture::admitted(node, capture_name, RawCaptureValue::node());
            let observation = self
                .ctx
                .type_ctx
                .records_raw_output_provenance()
                .then(|| raw.observation(capture_type.raw_intent(self.source)));
            let field = self.finish_capture_type(raw, capture_type);
            if let Some(observation) = observation {
                self.ctx.type_ctx.record_raw_capture_observation(
                    Pattern::CapturedPattern(node.clone()),
                    observation.emitting(field),
                );
            }
            return PatternShape::new(
                RootExtent::SingleNode,
                PatternFlow::Fields(self.ctx.type_ctx.intern_single_field(capture_name, field)),
            );
        };
        let inner = cap.wrap(inner);

        // Determine how inner flow relates to capture (e.g., ? makes field optional)
        let captured_inner = self.resolve_capture_inner(&inner);
        let inner_info = captured_inner.info;

        // A void inner that doesn't match exactly one node has no single node
        // for the capture to bind. Recover as `Node` — the error is already
        // reported. Direct quantifiers are exempt: the captured-quantifier
        // machinery defines their value (array, or optional node), and the
        // exactly-one check runs on their element instead.
        if !matches!(inner.node(), Pattern::QuantifiedPattern(_))
            && !self.report_capture_on_void_ref(inner.node(), &inner_info)
        {
            self.report_capture_without_single_node(inner.node(), &inner_info);
        }

        // Only the `Node` mechanism captures the matched node and lets the inner's
        // fields bubble up alongside (e.g. `(named (child) @c) @cap`). Every other
        // mechanism owns the inner's fields, so they must not also bubble. Sharing
        // the classifier with emission keeps the declared type and the effects in
        // lockstep.
        let mechanism = self.ctx.type_ctx.in_progress().capture_kind(
            inner.node(),
            self.ctx.dependency_analysis,
            self.ctx.interner,
        );
        let should_merge_fields =
            mechanism == CaptureKind::Node && matches!(&inner_info.flow, PatternFlow::Fields(_));

        let base = self.captured_base_type(inner.node(), &inner_info, should_merge_fields);
        let raw_field = FieldInfo::with_optional(base, captured_inner.makes_field_optional);
        let destination = self.capture_field_destination(
            capture_name,
            &inner_info,
            should_merge_fields,
            name_tok.text_range(),
        );
        // A Node capture owns only its matched node. Diagnostics from child
        // captures that bubble beside it do not invalidate that node value or
        // hide this capture's own capture-type diagnostics. Structured/list
        // captures own their inner output, so an error in that output does
        // invalidate their raw contract.
        let inner_has_capture_error = !matches!(inner.node(), Pattern::QuantifiedPattern(_))
            && inner_info.flow.is_void()
            && (inner_info.root_extent == RootExtent::Other
                || matches!(inner.node(), Pattern::DefRef(_)));
        let owned_inner_error = mechanism != CaptureKind::Node
            && self.ctx.diag.error_count() != errors_before_raw_capture;
        let raw_capture_valid =
            destination.admits_capture() && !inner_has_capture_error && !owned_inner_error;
        let emits_field = destination.admits_capture();
        let zero_node_terminal =
            !raw_field.optional && self.pattern_can_match_zero_nodes(inner.node());
        let raw = RawCapture::after_validation(
            node,
            capture_name,
            RawCaptureValue::inferred(mechanism, raw_field, zero_node_terminal),
            raw_capture_valid,
        );
        let observation = self
            .ctx
            .type_ctx
            .records_raw_output_provenance()
            .then(|| raw.observation(capture_type.raw_intent(self.source)));
        let field_info = self.finish_capture_type(raw, capture_type);
        if let Some(observation) = observation {
            let observation = if emits_field {
                observation.emitting(field_info)
            } else {
                observation
            };
            self.ctx.type_ctx.record_raw_capture_observation(
                Pattern::CapturedPattern(node.clone()),
                observation,
            );
        }
        let flow = destination.finish(self.ctx.type_ctx, capture_name, field_info);

        PatternShape::new(inner_info.root_extent, flow)
    }

    /// `:: TypeName` — name a structured capture or alias its semantic leaf.
    /// Recurses into arrays and optionals so the name lands on the element.
    /// Every occurrence is recorded for the naming pass to validate.
    fn apply_custom_capture_type(
        &mut self,
        type_id: TypeId,
        name: Symbol,
        range: TextRange,
    ) -> TypeId {
        match self.ctx.type_ctx.in_progress().type_shape(type_id).cloned() {
            Some(TypeShape::Struct(_) | TypeShape::Variant(_)) => {
                self.ctx
                    .type_ctx
                    .record_custom_capture_type(CustomCaptureTypeOccurrence {
                        name,
                        span: Span::new(self.source, range),
                        type_id,
                    });
                type_id
            }
            Some(TypeShape::Array { element, non_empty }) => {
                let element = self.apply_custom_capture_type(element, name, range);
                self.ctx
                    .type_ctx
                    .intern_type(TypeShape::Array { element, non_empty })
            }
            Some(TypeShape::Optional(inner)) => {
                let inner = self.apply_custom_capture_type(inner, name, range);
                self.ctx.type_ctx.intern_type(TypeShape::Optional(inner))
            }
            // A recursive reference keeps its definition's type; the naming
            // pass warns that the capture type is inert.
            Some(TypeShape::Ref(_)) => {
                self.ctx
                    .type_ctx
                    .record_custom_capture_type(CustomCaptureTypeOccurrence {
                        name,
                        span: Span::new(self.source, range),
                        type_id,
                    });
                type_id
            }
            // A custom leaf capture type is a nominal alias for Node.
            Some(TypeShape::Node | TypeShape::Custom(_)) => {
                let custom = self.ctx.type_ctx.intern_custom(name);
                self.ctx
                    .type_ctx
                    .record_custom_capture_type(CustomCaptureTypeOccurrence {
                        name,
                        span: Span::new(self.source, range),
                        type_id: custom,
                    });
                custom
            }
            Some(TypeShape::Text | TypeShape::Bool) => {
                unreachable!("ordinary captures cannot produce scalar roots")
            }
            // Recovery-only void falls back to a Node alias, matching the raw
            // capture's recovery type.
            _ => {
                let custom = self.ctx.type_ctx.intern_custom(name);
                self.ctx
                    .type_ctx
                    .record_custom_capture_type(CustomCaptureTypeOccurrence {
                        name,
                        span: Span::new(self.source, range),
                        type_id: custom,
                    });
                custom
            }
        }
    }

    fn resolve_capture_type(&mut self, capture: &CapturedPattern) -> ResolvedCaptureType {
        let Some(syntax) = capture.capture_type() else {
            return ResolvedCaptureType::None;
        };
        let Some(name) = syntax.name() else {
            return ResolvedCaptureType::Invalid;
        };
        let name_text = name.text();
        if let Some(built_in) = BuiltInCaptureType::parse(name_text) {
            return ResolvedCaptureType::BuiltIn(built_in, syntax.text_range());
        }
        if name_text
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_uppercase())
        {
            return ResolvedCaptureType::Custom(
                self.ctx.interner.intern(name_text),
                name.text_range(),
            );
        }

        self.report_unknown_capture_type(name_text, name.text_range());
        ResolvedCaptureType::Invalid
    }

    fn report_unknown_capture_type(&mut self, name: &str, range: TextRange) {
        let mut report = self
            .report(DiagnosticKind::UnknownCaptureType, range)
            .detail(name);

        if let Some(replacement) = suggested_builtin_capture_type(name) {
            report = report.fix(format!("use `:: {replacement}`"), replacement);
        }

        report
            .hint(
                "write `:: str`, `:: bool`, or a PascalCase custom capture type such as `:: MyType`",
            )
            .emit();
    }

    fn finish_capture_type(
        &mut self,
        raw: RawCapture,
        capture_type: ResolvedCaptureType,
    ) -> FieldInfo {
        let pattern = Pattern::CapturedPattern(raw.occurrence.clone());
        let raw_fact = raw.fact();
        let ordinary = || CaptureFact::ordinary(raw_fact.kind());

        let field = match capture_type {
            ResolvedCaptureType::Custom(name, range) if raw.valid => {
                let type_id = self.apply_custom_capture_type(raw.value.field.type_id, name, range);
                FieldInfo::with_optional(type_id, raw.value.field.optional)
            }
            ResolvedCaptureType::BuiltIn(_, _)
            | ResolvedCaptureType::Custom(_, _)
            | ResolvedCaptureType::Invalid
            | ResolvedCaptureType::None => raw.value.field,
        };
        self.ctx.type_ctx.record_capture_fact(pattern, ordinary());
        field
    }

    fn pattern_can_match_zero_nodes(&self, pattern: &Pattern) -> bool {
        crate::compiler::analyze::nullability::pattern_nullable(
            pattern,
            self.ctx.nullable_defs,
            self.ctx.dependency_analysis,
            self.ctx.interner,
        )
    }

    /// The capture's base type, before its custom capture type is applied.
    fn captured_base_type(
        &mut self,
        inner: &Pattern,
        inner_info: &PatternShape,
        should_merge_fields: bool,
    ) -> TypeId {
        if should_merge_fields {
            // Named node with bubbling children: the capture takes the matched node,
            // and the children bubble up alongside it.
            return TYPE_NODE;
        }

        self.determine_captured_base_type(inner, inner_info)
    }

    fn capture_field_destination(
        &mut self,
        capture_name: Symbol,
        inner_info: &PatternShape,
        should_merge_fields: bool,
        range: TextRange,
    ) -> CaptureFieldDestination {
        if !should_merge_fields {
            return CaptureFieldDestination::OwnScope;
        }

        let PatternFlow::Fields(type_id) = &inner_info.flow else {
            unreachable!("node captures only merge field flow");
        };
        let fields = self
            .ctx
            .type_ctx
            .in_progress()
            .expect_struct_fields(*type_id)
            .clone();
        let admits_capture = !fields.contains_key(&capture_name);
        if !admits_capture {
            let field = self.ctx.interner.resolve(capture_name).to_string();
            self.report(DiagnosticKind::DuplicateCaptureInScope, range)
                .detail(field)
                .emit();
        }

        CaptureFieldDestination::Bubbling {
            fields,
            admits_capture,
        }
    }

    /// Logic for how quantifier on the inner expression affects the capture field.
    fn resolve_capture_inner(&mut self, inner: &Located<Pattern>) -> CaptureInner {
        if let Pattern::QuantifiedPattern(q) = inner.node() {
            let quantifier = self.quantifier_kind(q);
            let located = inner.wrap(q.clone());
            let info = self.infer_quantified_pattern_in(&located, QuantifiedContext::Captured);
            CaptureInner {
                info,
                // `?` makes the resulting capture field optional; `*` and `+`
                // collect record elements into a list that is always present.
                makes_field_optional: quantifier == QuantifierKind::Optional,
            }
        } else {
            CaptureInner {
                info: self.infer_pattern_value(inner),
                makes_field_optional: false,
            }
        }
    }

    /// The capture's base type from the inner flow, before any capture type.
    fn determine_captured_base_type(
        &mut self,
        inner: &Pattern,
        inner_info: &PatternShape,
    ) -> TypeId {
        match &inner_info.flow {
            // A truly empty scope (`{}`) captures an empty struct; any other void
            // capture is the matched node.
            PatternFlow::Void => {
                if is_empty_group(inner) {
                    let empty = self.ctx.type_ctx.intern_struct(BTreeMap::new());
                    let span = Span::new(self.source, inner.text_range());
                    self.ctx.type_ctx.record_type_provenance(empty, span);
                    empty
                } else {
                    TYPE_NODE
                }
            }
            PatternFlow::Value(type_id) | PatternFlow::Fields(type_id) => *type_id,
        }
    }

    fn infer_quantified_pattern_in(
        &mut self,
        quant: &Located<QuantifiedPattern>,
        context: QuantifiedContext,
    ) -> PatternShape {
        let pattern = Pattern::QuantifiedPattern(quant.node().clone());
        if let Some(info) = self.ctx.type_ctx.in_progress().pattern_result(&pattern) {
            return info.clone();
        }

        let info = self.compute_quantified_pattern(quant, context);
        self.record_result(&quant.wrap(pattern), info)
    }

    fn compute_quantified_pattern(
        &mut self,
        quant: &Located<QuantifiedPattern>,
        context: QuantifiedContext,
    ) -> PatternShape {
        let Some(inner) = quant.node().inner() else {
            return PatternShape::void();
        };
        let inner = quant.wrap(inner);

        let inner_info = match context {
            QuantifiedContext::Captured | QuantifiedContext::DefinitionValue => {
                self.infer_pattern_value(&inner)
            }
            QuantifiedContext::Discard => self.infer_pattern_discarded(&inner),
            QuantifiedContext::Bare => self.infer_pattern(&inner),
        };
        let quantifier = self.quantifier_kind(quant.node());

        let flow = match quantifier {
            QuantifierKind::Optional => match context {
                // A captured `?` of a multi-node void group has no single node
                // to bind (or null), just like a captured repeat. Otherwise the
                // inner flow passes through untouched: the capture collects it
                // as one nullable value — fields keep their true modality, the
                // null lives on the capture field alone.
                QuantifiedContext::Captured => {
                    self.report_multi_element_scalar(quant.node(), &inner_info);
                    inner_info.flow
                }
                // Internal captures of a bare `?` have nothing to collect them,
                // exactly like a bare repeat: a skip would scatter correlated
                // nulls into the enclosing scope. Recover with the legacy
                // bubbling shape so downstream inference stays coherent.
                QuantifiedContext::Bare => {
                    self.report_internal_capture_dimensionality(quant.node(), &inner_info);
                    self.make_flow_optional(inner_info.flow)
                }
                QuantifiedContext::Discard => PatternFlow::Void,
                // The definition collects the skip as its own null: the output
                // is the optional type itself, not a field-optionality flag.
                QuantifiedContext::DefinitionValue => {
                    let element = self.definition_element_type(quant.node(), &inner, &inner_info);
                    PatternFlow::Value(self.ctx.type_ctx.intern_type(TypeShape::Optional(element)))
                }
            },
            QuantifierKind::ZeroOrMore | QuantifierKind::OneOrMore => {
                // A value-collecting repeat over a zero-width-capable element
                // could complete an iteration without advancing; reject before
                // lowering has to give the loop an exit it cannot have.
                if matches!(
                    context,
                    QuantifiedContext::Captured | QuantifiedContext::DefinitionValue
                ) {
                    self.reject_zero_width_repeat(quant.node(), &inner);
                }
                if context == QuantifiedContext::DefinitionValue {
                    let element = self.definition_element_type(quant.node(), &inner, &inner_info);
                    PatternFlow::Value(self.ctx.type_ctx.intern_type(TypeShape::Array {
                        element,
                        non_empty: quantifier.is_non_empty(),
                    }))
                } else {
                    self.check_quantified_array_dimensionality(quant.node(), &inner_info, context);
                    self.make_flow_array(inner_info.flow, quantifier.is_non_empty(), context)
                }
            }
        };

        // One match of a quantified pattern has variable top-level extent,
        // regardless of its element's extent.
        PatternShape::new(RootExtent::Other, flow)
    }

    fn check_quantified_array_dimensionality(
        &mut self,
        quant: &QuantifiedPattern,
        inner_info: &PatternShape,
        context: QuantifiedContext,
    ) {
        match context {
            // Repeated captures with no list to land in.
            QuantifiedContext::Bare => {
                self.report_internal_capture_dimensionality(quant, inner_info);
            }
            // A captured repeat of a multi-node void group has no defined
            // element value.
            QuantifiedContext::Captured => {
                self.report_multi_element_scalar(quant, inner_info);
            }
            // Everything is discarded; there is nothing to collect wrongly.
            QuantifiedContext::Discard => {}
            QuantifiedContext::DefinitionValue => {
                unreachable!("quantifier-rooted definitions resolve their element type instead")
            }
        }
    }

    /// Reject `*`/`+` whose element is a reference to an optional- or
    /// array-rooted definition that can match zero nodes: a zero-width
    /// iteration completes without consuming, so the loop collects a spurious
    /// null/empty element at every non-matching candidate. Scoped to
    /// wrapper-shaped outputs — the surface quantifier-rooted definitions
    /// introduce — so nullable struct-valued definitions (a captured `?` at
    /// the root) keep their existing repeat behavior.
    fn reject_zero_width_repeat(&mut self, quant: &QuantifiedPattern, inner: &Located<Pattern>) {
        let mut element = inner.node().clone();
        while let Pattern::FieldPattern(f) = &element {
            match f.value() {
                Some(v) => element = v,
                None => return,
            }
        }
        let Pattern::DefRef(r) = &element else {
            return;
        };
        let Some(name) = r.name() else {
            return;
        };
        let Some(def_id) = self
            .ctx
            .dependency_analysis
            .def_id_for_name(self.ctx.interner, name.text())
        else {
            return;
        };
        if !self.ctx.nullable_defs.contains(&def_id) {
            return;
        }
        // Mid-SCC targets have no registered output yet; the recursion checks
        // own those cycles.
        let view = self.ctx.type_ctx.in_progress();
        let wrapper_output = view.def_output(def_id).is_some_and(|output| {
            matches!(
                view.type_shape(output),
                Some(TypeShape::Optional(_) | TypeShape::Array { .. })
            )
        });
        if wrapper_output {
            self.report_zero_width_repeat(quant, &element);
        }
    }

    /// Resolve the element type of a quantifier-rooted definition body.
    ///
    /// The definition names its output — the container — so the element must
    /// be a type that needs no fresh name: a matched node (void inner) or
    /// another definition's output (a reference). Anonymous element shapes — a
    /// record of captures or a labeled alternation — have no name source (names
    /// come only from defs, captures, custom capture types, and case tags) and are
    /// rejected with a hint to split the element into its own definition. The
    /// plausible element type is still returned so downstream inference isn't
    /// poisoned by void.
    fn definition_element_type(
        &mut self,
        quant: &QuantifiedPattern,
        inner: &Located<Pattern>,
        inner_info: &PatternShape,
    ) -> TypeId {
        match &inner_info.flow {
            PatternFlow::Void => {
                self.report_multi_element_scalar(quant, inner_info);
                TYPE_NODE
            }
            PatternFlow::Value(t) => {
                if labeled_alternation_value_root(inner.node()) {
                    self.report_unnamed_quantified_element(quant, "a labeled alternation");
                }
                *t
            }
            PatternFlow::Fields(t) => {
                self.report_unnamed_quantified_element(quant, "an anonymous record of captures");
                *t
            }
        }
    }

    fn make_flow_optional(&mut self, flow: PatternFlow) -> PatternFlow {
        match flow {
            PatternFlow::Void => PatternFlow::Void,
            PatternFlow::Value(t) => {
                PatternFlow::Value(self.ctx.type_ctx.intern_type(TypeShape::Optional(t)))
            }
            PatternFlow::Fields(type_id) => {
                let optional_fields: BTreeMap<_, _> = self
                    .ctx
                    .type_ctx
                    .in_progress()
                    .expect_struct_fields(type_id)
                    .iter()
                    .map(|(&k, &v)| (k, v.make_optional()))
                    .collect();
                PatternFlow::Fields(self.ctx.type_ctx.intern_struct(optional_fields))
            }
        }
    }

    fn make_flow_array(
        &mut self,
        flow: PatternFlow,
        non_empty: bool,
        context: QuantifiedContext,
    ) -> PatternFlow {
        let intern_array = |ctx: &mut TypeAnalysisBuilder, element: TypeId| {
            PatternFlow::Value(ctx.intern_type(TypeShape::Array { element, non_empty }))
        };

        match (context, flow) {
            // A bare repeat is structural: no output context observes its
            // values, so a void or unobserved value produces nothing. A
            // discarded repeat produces nothing outright.
            (QuantifiedContext::Bare, PatternFlow::Void | PatternFlow::Value(_))
            | (QuantifiedContext::Discard, _) => PatternFlow::Void,
            // Bare with bubbling captures: `report_internal_capture_dimensionality`
            // already errored. Produce the plausible array type anyway so
            // downstream inference isn't poisoned by void.
            (QuantifiedContext::Bare, PatternFlow::Fields(struct_type)) => {
                intern_array(self.ctx.type_ctx, struct_type)
            }
            // Captured repeats collect elements: matched nodes, pending values
            // (variant/reference results), or records of captured fields.
            (QuantifiedContext::Captured, PatternFlow::Void) => {
                intern_array(self.ctx.type_ctx, TYPE_NODE)
            }
            (
                QuantifiedContext::Captured,
                PatternFlow::Value(element) | PatternFlow::Fields(element),
            ) => intern_array(self.ctx.type_ctx, element),
            (QuantifiedContext::DefinitionValue, _) => {
                unreachable!("quantifier-rooted definitions resolve their element type instead")
            }
        }
    }

    /// A grammar-field constraint occupies one child position and delegates its
    /// result flow to the constrained pattern.
    fn infer_field_pattern_in(
        &mut self,
        field: &Located<FieldPattern>,
        output: OutputContext,
    ) -> PatternShape {
        let Some(value) = field.node().value() else {
            return PatternShape::void();
        };
        let value = field.wrap(value);

        let value_info = match output {
            OutputContext::Fields => self.infer_pattern(&value),
            OutputContext::Value => self.infer_pattern_value(&value),
            OutputContext::Discard => self.infer_pattern_discarded(&value),
        };

        // A field names exactly one child per match. Under any quantifier/capture
        // wrappers (`f: (x)*` repeats the whole field), the constrained value must
        // be a single node: a sequence `{...}` never is — even holding one element,
        // the spec restricts field values to a node, an alternation, or a quantifier
        // of those — and a value matching many nodes never is either.
        let core = Self::field_value_core(value.node());
        if matches!(core, Pattern::SeqPattern(_))
            || self.core_root_extent(&core, &value_info) == RootExtent::Other
        {
            self.report_field_requires_single_node(field.node(), value.node());
        }

        PatternShape::new(RootExtent::SingleNode, value_info.flow)
    }

    /// The field value under its capture/quantifier wrappers. `f: (x)* @c`
    /// parses as `(f: (x)) * @c`, so those wrappers bind the field, not the
    /// value; strip them to reach the pattern the field actually constrains.
    fn field_value_core(value: &Pattern) -> Pattern {
        let mut core = value.clone();
        loop {
            let inner = match &core {
                Pattern::CapturedPattern(c) => c.inner(),
                Pattern::QuantifiedPattern(q) => q.inner(),
                _ => None,
            };
            match inner {
                Some(inner) => core = inner,
                None => return core,
            }
        }
    }

    /// The root extent of an already-inferred field-value core. Its result was
    /// cached while inferring the value.
    fn core_root_extent(&self, core: &Pattern, value_info: &PatternShape) -> RootExtent {
        self.ctx
            .type_ctx
            .in_progress()
            .pattern_result(core)
            .map(|info| info.root_extent)
            .unwrap_or(value_info.root_extent)
    }

    fn quantifier_kind(&self, quant: &QuantifiedPattern) -> QuantifierKind {
        // Shared with `TypeAnalysis::capture_kind` and `compile`'s implicit-array gate so the
        // three never disagree on a quantifier's arity.
        quant
            .quantifier_kind()
            .expect("quantifier kind resolved before inference")
    }
}

/// How a pattern contributes to its enclosing result. Grammar-field constraints
/// thread the context through because they navigate to a pattern without
/// creating a result boundary.
#[derive(Clone, Copy, PartialEq, Eq)]
enum OutputContext {
    Fields,
    Value,
    Discard,
}

/// Whether a definition root is a labeled alternation, possibly reached through
/// grammar-field constraints. Such a root produces the definition's variant
/// value; a bare reference remains structural.
fn labeled_alternation_value_root(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::Alternation(alternation) => alternation.labeling() == Labeling::Labeled,
        Pattern::FieldPattern(f) => f
            .value()
            .is_some_and(|v| labeled_alternation_value_root(&v)),
        _ => false,
    }
}

/// Whether a definition root directly supplies a value: a labeled alternation
/// supplies a variant, while a quantifier supplies a list or option. Grammar-
/// field constraints forward the root role to their value.
///
/// Shared with lowering, which keys its pending-value emission on the same
/// predicate: a `Value`-flow pattern emits producer effects only where a
/// definition or capture materializes the value. Diverging answers
/// would make the bytecode effect-stack verifier reject valid queries.
pub(crate) fn definition_value_root(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::Alternation(alternation) => alternation.labeling() == Labeling::Labeled,
        Pattern::QuantifiedPattern(_) => true,
        Pattern::FieldPattern(f) => f.value().is_some_and(|v| definition_value_root(&v)),
        _ => false,
    }
}

pub(super) struct InferPassEnv<'a, 'd> {
    pub interner: &'a mut Interner,
    pub symbol_table: &'a SymbolTable,
    pub dependency_analysis: &'a DependencyAnalysis,
    pub structural_facts: &'a StructuralFacts,
    pub diag: &'d mut Diagnostics,
}

/// Syntax-only fixpoints computed once before raw inference. Capture-type
/// normalization reads the builtin-only provenance projection and never
/// recomputes them.
pub(super) struct StructuralFacts {
    nullable_defs: HashSet<DefId>,
    definition_root_extents: HashMap<DefId, RootExtent>,
}

impl StructuralFacts {
    pub fn analyze(
        interner: &Interner,
        symbol_table: &SymbolTable,
        dependency_analysis: &DependencyAnalysis,
    ) -> Self {
        Self {
            nullable_defs: compute_nullable_defs(interner, symbol_table, dependency_analysis),
            definition_root_extents: super::root_extent::compute_definition_root_extents(
                interner,
                symbol_table,
                dependency_analysis,
            ),
        }
    }
}

/// Orchestrates type inference across all definitions in dependency order.
pub(super) struct InferPass<'a, 'd> {
    ctx: TypeAnalysisBuilder,
    analysis: InferPassEnv<'a, 'd>,
}

impl<'a, 'd> InferPass<'a, 'd> {
    pub fn new(analysis: InferPassEnv<'a, 'd>) -> Self {
        Self {
            ctx: TypeAnalysisBuilder::new(),
            analysis,
        }
    }

    pub fn normalizing_capture_types(analysis: InferPassEnv<'a, 'd>) -> Self {
        Self {
            ctx: TypeAnalysisBuilder::for_capture_normalization(),
            analysis,
        }
    }

    pub fn run(mut self) -> TypeAnalysisBuilder {
        // Definition root extents are a syntactic fixpoint, computed up front
        // so references always report their target's real extent, including
        // through recursion. Only output types need SCC deferral.
        for (&def_id, &extent) in &self.analysis.structural_facts.definition_root_extents {
            self.ctx.record_def_root_extent(def_id, extent);
        }

        // Definition identity (names, DefIds) is owned by DependencyAnalysis and
        // read from there; the builder only accumulates inferred types.
        self.process_sccs();
        self.assert_all_definitions_processed();
        self.check_in_progress_reference_captures();
        self.ctx
    }

    fn visit<T>(
        &mut self,
        source: SourceId,
        visit: impl FnOnce(&mut InferVisitor<'_, '_>) -> T,
    ) -> T {
        let state = InferState {
            type_ctx: &mut self.ctx,
            interner: self.analysis.interner,
            symbol_table: self.analysis.symbol_table,
            dependency_analysis: self.analysis.dependency_analysis,
            nullable_defs: &self.analysis.structural_facts.nullable_defs,
            diag: &mut *self.analysis.diag,
        };
        let mut visitor = InferVisitor::new(state, source);
        visit(&mut visitor)
    }

    /// Process definitions in SCC order (leaves first).
    fn process_sccs(&mut self) {
        for scc in self.analysis.dependency_analysis.sccs() {
            for &def_id in scc {
                let def_name = self
                    .analysis
                    .interner
                    .resolve(self.analysis.dependency_analysis.def_name_sym(def_id))
                    .to_owned();
                let source_id = self.analysis.dependency_analysis.def_source_id(def_id);
                self.infer_and_register(def_id, &def_name, source_id);
            }
        }
    }

    fn assert_all_definitions_processed(&mut self) {
        for name in self.analysis.symbol_table.names() {
            let def_id = self
                .analysis
                .dependency_analysis
                .def_id_for_name(self.analysis.interner, name)
                .expect("dependency analysis must assign every definition a DefId");
            assert!(
                self.ctx.in_progress().def_output(def_id).is_some(),
                "dependency analysis must schedule every definition before type analysis",
            );
        }
    }

    fn infer_and_register(&mut self, def_id: DefId, def_name: &str, source_id: SourceId) {
        let body = self
            .analysis
            .symbol_table
            .body(def_name)
            .cloned()
            .expect("symbol-table source entry must have a body");

        // Infer this definition's body only; references into other definitions
        // resolve to their precomputed results.
        let located_body = Located::new(source_id, body.clone());
        let info = self.visit(source_id, |visitor| {
            visitor.infer_pattern_value(&located_body)
        });

        let type_id = match &info.flow {
            PatternFlow::Void => TYPE_VOID,
            PatternFlow::Fields(t) => *t,
            // A root value is the definition's result only when a labeled
            // alternation or quantifier supplies it directly. A bare reference
            // is structural: no capture, no output — the definition still
            // matches, like a capture-less regex.
            PatternFlow::Value(t) => {
                if definition_value_root(&body) {
                    *t
                } else {
                    TYPE_VOID
                }
            }
        };
        self.ctx.record_def_output(def_id, type_id);
        let value_role = if definition_value_root(&body) {
            RawDefinitionValueRole::Value
        } else {
            RawDefinitionValueRole::Fields
        };
        self.ctx.record_raw_definition(def_id, &body, value_role);

        let precomputed = self
            .ctx
            .def_root_extent(def_id)
            .expect("definition root extents are precomputed before inference");
        assert_eq!(
            info.root_extent, precomputed,
            "definition root-extent pre-pass must agree with inference",
        );
    }
}

pub(super) fn freeze_field_completions(types: &mut TypeAnalysisBuilder) {
    for (pattern, type_id) in types.alternation_field_results() {
        let fields = types.in_progress().expect_struct_fields(type_id).clone();
        let alternative_fields = alternation_alternative_fields(types, &pattern);
        let completions = fields
            .into_iter()
            .map(|(name, info)| {
                let omitted = alternative_fields
                    .iter()
                    .any(|fields| !fields.contains(&name));
                let completion = if !omitted {
                    FieldCompletion::AlwaysPresent
                } else if !info.optional
                    && matches!(
                        types.in_progress().type_shape(info.type_id),
                        Some(TypeShape::Array { .. })
                    )
                {
                    FieldCompletion::EmptyList
                } else {
                    FieldCompletion::Absent
                };
                (name, completion)
            })
            .collect();
        types.record_field_completions(pattern, FieldCompletions::new(completions));
    }
}

fn alternation_alternative_fields(
    types: &TypeAnalysisBuilder,
    pattern: &Pattern,
) -> Vec<HashSet<Symbol>> {
    let bodies: Vec<Option<Pattern>> = match pattern {
        Pattern::Alternation(alternation) => alternation
            .alternatives()
            .map(|alternative| alternative.body())
            .chain(alternation.patterns().map(Some))
            .collect(),
        _ => unreachable!("field completions are only built for alternations"),
    };

    bodies
        .into_iter()
        .map(|body| {
            let Some(body) = body else {
                return HashSet::new();
            };
            let view = types.in_progress();
            let Some(shape) = view.pattern_result(&body) else {
                return HashSet::new();
            };
            let PatternFlow::Fields(type_id) = shape.flow else {
                return HashSet::new();
            };
            view.expect_struct_fields(type_id).keys().copied().collect()
        })
        .collect()
}
