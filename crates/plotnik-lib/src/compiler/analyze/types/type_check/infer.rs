//! Bottom-up type inference visitor.
//!
//! Traverses the AST and computes PatternShape (Arity + PatternFlow) for each expression.
//! Reports diagnostics for type errors like strict dimensionality violations.

use std::collections::BTreeMap;

use crate::core::{Interner, Symbol};
use rowan::TextRange;

use super::unify::unify_flows;
use crate::compiler::analyze::types::capture_kind::CaptureKind;
use crate::compiler::analyze::types::type_analysis::{TypeAnalysis, TypeAnalysisBuilder};
use crate::compiler::analyze::types::type_shape::{
    Arity, FieldInfo, PatternFlow, PatternShape, QuantifierKind, TYPE_NODE, TYPE_VOID, TypeId,
    TypeShape,
};

use crate::compiler::analyze::Located;
use crate::compiler::analyze::names::SymbolTable;
use crate::compiler::analyze::refs::DependencyAnalysis;
use crate::compiler::diagnostics::report::{DiagnosticBuilder, DiagnosticKind, Diagnostics};
use crate::compiler::diagnostics::source::SourceId;
use crate::compiler::diagnostics::span::Span;
use crate::compiler::ids::DefId;
use crate::compiler::parse::ast::{
    Branch, CapturedPattern, DefRef, EnumPattern, FieldPattern, NodePattern, Pattern,
    QuantifiedPattern, SeqPattern, TokenPattern, UnionPattern, is_empty_group,
};

mod diagnostics;
mod flow;

/// Shared state for a single inference pass over the AST.
pub struct InferState<'a, 'd> {
    pub type_ctx: &'a mut TypeAnalysisBuilder,
    pub interner: &'a mut Interner,
    pub symbol_table: &'a SymbolTable,
    pub dependency_analysis: &'a DependencyAnalysis,
    pub(crate) diag: &'d mut Diagnostics,
}

/// Inference visitor for a single pass over the AST.
pub struct InferVisitor<'a, 'd> {
    ctx: InferState<'a, 'd>,
    source: SourceId,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum QuantifiedCaptureMode {
    Bare,
    RowCapture,
}

struct CaptureInner {
    info: PatternShape,
    makes_field_optional: bool,
}

struct ChildFlow {
    merged_fields: BTreeMap<Symbol, FieldInfo>,
    output_children: Vec<(TextRange, TypeId)>,
}

enum RefFlowBoundary {
    Transparent,
    RecursiveEnumValue,
    RecursiveOpaque,
}

fn flow_to_type(flow: &PatternFlow) -> TypeId {
    match flow {
        PatternFlow::Void => TYPE_VOID,
        PatternFlow::Value(t) | PatternFlow::Fields(t) => *t,
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
        self.ctx
            .type_ctx
            .record_pattern_result(pattern.node().clone(), info.clone());
        info
    }

    fn compute_pattern(&mut self, pattern: &Located<Pattern>) -> PatternShape {
        match pattern.node() {
            Pattern::NodePattern(n) => self.infer_named_node(&pattern.wrap(n.clone())),
            Pattern::TokenPattern(n) => self.infer_anonymous_node(n),
            Pattern::DefRef(r) => self.infer_ref(r),
            Pattern::SeqPattern(s) => self.infer_seq_pattern(&pattern.wrap(s.clone())),
            Pattern::Union(u) => self.infer_union(&pattern.wrap(u.clone())),
            Pattern::Enum(e) => self.infer_enum(&pattern.wrap(e.clone())),
            Pattern::CapturedPattern(c) => self.infer_captured_pattern(&pattern.wrap(c.clone())),
            Pattern::QuantifiedPattern(q) => {
                self.infer_quantified_pattern(&pattern.wrap(q.clone()))
            }
            Pattern::FieldPattern(f) => self.infer_field_pattern(&pattern.wrap(f.clone())),
        }
    }

    /// Named node: matches one position, bubbles up child captures or propagates output.
    fn infer_named_node(&mut self, node: &Located<NodePattern>) -> PatternShape {
        let children = node.node().children().map(|child| node.wrap(child));
        let child_flow = self.collect_child_flow(children);

        let flow = self.compute_merged_flow(
            child_flow.merged_fields,
            child_flow.output_children,
            node.node().text_range(),
        );
        PatternShape::new(Arity::One, flow)
    }

    /// Anonymous node (literal or wildcard): matches one position, produces nothing.
    fn infer_anonymous_node(&mut self, _node: &TokenPattern) -> PatternShape {
        PatternShape::new(Arity::One, PatternFlow::Void)
    }

    /// Reference: transparent for non-recursive defs, opaque boundary for recursive ones.
    ///
    /// A non-recursive ref resolves to its target's already-computed result rather
    /// than descending into the body. Definitions are processed in reverse-topological
    /// SCC order (leaves first), so a non-recursive target is always computed before
    /// any referrer — the body is never re-walked, and its diagnostics stay attributed
    /// to its own definition's pass (and source).
    fn infer_ref(&mut self, r: &DefRef) -> PatternShape {
        let Some(name_tok) = r.name() else {
            return PatternShape::void();
        };
        let name = name_tok.text();
        let name_sym = self.ctx.interner.intern(name);

        // No definition: an undefined reference, already diagnosed upstream
        // (`UndefinedReference`). Outside the trust boundary — answer with void.
        let Some(body) = self.ctx.symbol_table.body(name) else {
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

        match self.ref_flow_boundary(def_id, body) {
            RefFlowBoundary::RecursiveEnumValue => {
                let ref_type = self.ctx.type_ctx.intern_type(TypeShape::Ref(def_id));
                PatternShape::new(Arity::One, PatternFlow::Value(ref_type))
            }
            RefFlowBoundary::RecursiveOpaque => PatternShape::new(Arity::One, PatternFlow::Void),
            RefFlowBoundary::Transparent => {
                // Non-recursive refs are transparent: return the target's precomputed
                // result so the enclosing scope sees its fields/arity exactly as if the
                // body were inlined here. SCC order guarantees it is already present.
                self.ctx.type_ctx.def_memo(def_id).cloned().expect(
                    "non-recursive reference target is inferred before the referrer (SCC order)",
                )
            }
        }
    }

    fn ref_flow_boundary(&self, def_id: DefId, body: &Pattern) -> RefFlowBoundary {
        if !self.ctx.dependency_analysis.is_recursive_def(def_id) {
            return RefFlowBoundary::Transparent;
        }

        // Recursive refs are opaque boundaries - they don't bubble captures.
        // Enum alternations are the exception because they always produce Enum output.
        if self.body_is_enum(body) {
            return RefFlowBoundary::RecursiveEnumValue;
        }

        RefFlowBoundary::RecursiveOpaque
    }

    /// An enum body always produces an Enum type (Value flow), so a recursive
    /// `Ref` to such a definition is safe to treat as `Value(Ref)` in uncaptured
    /// contexts rather than `Void`.
    fn body_is_enum(&self, body: &Pattern) -> bool {
        matches!(body, Pattern::Enum(_))
    }

    /// Sequence: Arity aggregation, strict field merging, and output propagation.
    fn infer_seq_pattern(&mut self, seq: &Located<SeqPattern>) -> PatternShape {
        let children: Vec<Located<Pattern>> = seq.node().children().map(|c| seq.wrap(c)).collect();

        let arity = self.compute_sequence_arity(&children);
        let child_flow = self.collect_child_flow(children.iter().cloned());

        let flow = self.compute_merged_flow(
            child_flow.merged_fields,
            child_flow.output_children,
            seq.node().text_range(),
        );
        PatternShape::new(arity, flow)
    }

    fn collect_child_flow(
        &mut self,
        children: impl IntoIterator<Item = Located<Pattern>>,
    ) -> ChildFlow {
        let mut merged_fields: BTreeMap<Symbol, FieldInfo> = BTreeMap::new();
        let mut output_children: Vec<(TextRange, TypeId)> = Vec::new();

        for child in children {
            let child_info = self.infer_pattern(&child);

            match &child_info.flow {
                PatternFlow::Fields(type_id) => {
                    let fields = self
                        .ctx
                        .type_ctx
                        .in_progress()
                        .expect_struct_fields(*type_id)
                        .clone();
                    self.merge_fields(&mut merged_fields, &fields, child.node().text_range());
                }
                PatternFlow::Value(type_id) => {
                    if self
                        .ctx
                        .type_ctx
                        .in_progress()
                        .is_structured_output(*type_id)
                    {
                        output_children.push((child.node().text_range(), *type_id));
                    }
                }
                PatternFlow::Void => {}
            }
        }

        ChildFlow {
            merged_fields,
            output_children,
        }
    }

    fn compute_sequence_arity(&mut self, children: &[Located<Pattern>]) -> Arity {
        match children {
            [] => Arity::One,
            [child] => self.infer_pattern(child).arity,
            _ => Arity::Many,
        }
    }

    fn infer_enum(&mut self, e: &Located<EnumPattern>) -> PatternShape {
        let mut variants: BTreeMap<Symbol, TypeId> = BTreeMap::new();
        let mut combined_arity = Arity::One;

        for branch in e.node().branches() {
            let label = branch.label().expect("enum branch must have a label");
            let label_sym = self.ctx.interner.intern(label.text());

            // A BTreeMap would silently collapse duplicate labels, leaving the enum
            // with fewer variants than the emitter expects. Reject them instead.
            if variants.contains_key(&label_sym) {
                self.report_duplicate_enum_label(label.text_range(), label.text());
                if let Some(body_info) = self.infer_enum_branch_body(e, &branch) {
                    combined_arity = combined_arity.combine(body_info.arity);
                }
                continue;
            }

            let Some(body_info) = self.infer_enum_branch_body(e, &branch) else {
                // Empty variant -> Void (no payload)
                variants.insert(label_sym, TYPE_VOID);
                continue;
            };

            combined_arity = combined_arity.combine(body_info.arity);
            variants.insert(label_sym, flow_to_type(&body_info.flow));
        }

        let enum_type = self.ctx.type_ctx.intern_type(TypeShape::Enum(variants));
        PatternShape::new(combined_arity, PatternFlow::Value(enum_type))
    }

    fn report_duplicate_enum_label(&mut self, range: TextRange, label: &str) {
        self.report(DiagnosticKind::DuplicateAlternationLabel, range)
            .detail(label)
            .emit();
    }

    fn infer_enum_branch_body(
        &mut self,
        e: &Located<EnumPattern>,
        branch: &Branch,
    ) -> Option<PatternShape> {
        branch.body().map(|body| self.infer_pattern(&e.wrap(body)))
    }

    fn infer_union(&mut self, union: &Located<UnionPattern>) -> PatternShape {
        let mut flows: Vec<PatternFlow> = Vec::new();
        let mut combined_arity = Arity::One;

        for branch in union.node().branches() {
            if let Some(body) = branch.body() {
                let info = self.infer_pattern(&union.wrap(body));
                combined_arity = combined_arity.combine(info.arity);
                flows.push(info.flow);
            }
        }

        for pattern in union.node().patterns() {
            let info = self.infer_pattern(&union.wrap(pattern));
            combined_arity = combined_arity.combine(info.arity);
            flows.push(info.flow);
        }

        let unified_flow = match unify_flows(self.ctx.type_ctx, flows) {
            Ok(flow) => flow,
            Err(err) => {
                self.report_unify_error(union.node(), &err);
                PatternFlow::Void
            }
        };

        PatternShape::new(combined_arity, unified_flow)
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

        // Suppressive captures don't contribute to output type
        if node.is_suppressive() {
            // Still infer inner for structural validation, but don't create fields
            return node
                .inner()
                .map(|i| self.infer_pattern(&cap.wrap(i)))
                .map(|info| PatternShape::new(info.arity, PatternFlow::Void))
                .unwrap_or_else(PatternShape::void);
        }

        let Some(name_tok) = node.name() else {
            // Recover gracefully
            return node
                .inner()
                .map(|i| self.infer_pattern(&cap.wrap(i)))
                .unwrap_or_else(PatternShape::void);
        };
        let capture_name = self.ctx.interner.intern(&name_tok.text()[1..]); // Strip @ prefix

        let annotation = self.resolve_annotation(node);

        let Some(inner) = node.inner() else {
            // Capture without inner -> a Node field (annotation may alias it).
            let type_id = annotation.map_or(TYPE_NODE, |name| self.annotate_named(TYPE_NODE, name));
            let field = FieldInfo::required(type_id);
            return PatternShape::new(
                Arity::One,
                PatternFlow::Fields(self.ctx.type_ctx.intern_single_field(capture_name, field)),
            );
        };
        let inner = cap.wrap(inner);

        // Determine how inner flow relates to capture (e.g., ? makes field optional)
        let captured_inner = self.resolve_capture_inner(&inner);
        let inner_info = captured_inner.info;

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
        let field_info =
            self.captured_field_info(base, annotation, captured_inner.makes_field_optional);
        let flow =
            self.captured_field_flow(capture_name, field_info, &inner_info, should_merge_fields);

        PatternShape::new(inner_info.arity, flow)
    }

    /// `:: TypeName` — name a structured capture (struct/enum) or alias a node.
    /// Recurses into arrays and optionals so the name lands on the element.
    fn annotate_named(&mut self, type_id: TypeId, name: Symbol) -> TypeId {
        match self.ctx.type_ctx.in_progress().type_shape(type_id).cloned() {
            Some(TypeShape::Struct(_) | TypeShape::Enum(_)) => {
                self.ctx.type_ctx.define_type_alias(type_id, name);
                type_id
            }
            Some(TypeShape::Array { element, non_empty }) => {
                let element = self.annotate_named(element, name);
                self.ctx
                    .type_ctx
                    .intern_type(TypeShape::Array { element, non_empty })
            }
            Some(TypeShape::Optional(inner)) => {
                let inner = self.annotate_named(inner, name);
                self.ctx.type_ctx.intern_type(TypeShape::Optional(inner))
            }
            // Node, recursive Ref, or void: a named alias to the value.
            _ => self.ctx.type_ctx.intern_type(TypeShape::Custom(name)),
        }
    }

    /// Resolves an explicit type annotation like `@foo :: TypeName` into the
    /// interned type name. Returns `None` when the capture has no annotation.
    fn resolve_annotation(&mut self, cap: &CapturedPattern) -> Option<Symbol> {
        cap.type_annotation()
            .and_then(|t| t.name())
            .map(|n| self.ctx.interner.intern(n.text()))
    }

    /// The capture's base type, before its `:: TypeName` annotation is applied.
    fn captured_base_type(
        &mut self,
        inner: &Pattern,
        inner_info: &PatternShape,
        should_merge_fields: bool,
    ) -> TypeId {
        if should_merge_fields {
            // Named node with bubbling children: the capture takes the matched node,
            // and the children bubble up alongside it.
            return self.recursive_ref_type(inner).unwrap_or(TYPE_NODE);
        }

        self.determine_captured_base_type(inner, inner_info)
    }

    fn captured_field_info(
        &mut self,
        base: TypeId,
        annotation: Option<Symbol>,
        is_optional: bool,
    ) -> FieldInfo {
        let captured_type = annotation.map_or(base, |name| self.annotate_named(base, name));

        FieldInfo::with_optional(captured_type, is_optional)
    }

    fn captured_field_flow(
        &mut self,
        capture_name: Symbol,
        field_info: FieldInfo,
        inner_info: &PatternShape,
        should_merge_fields: bool,
    ) -> PatternFlow {
        if !should_merge_fields {
            return PatternFlow::Fields(
                self.ctx
                    .type_ctx
                    .intern_single_field(capture_name, field_info),
            );
        }

        let PatternFlow::Fields(type_id) = &inner_info.flow else {
            unreachable!("node captures only merge field flow");
        };
        let mut fields = self
            .ctx
            .type_ctx
            .in_progress()
            .expect_struct_fields(*type_id)
            .clone();
        fields.insert(capture_name, field_info);

        PatternFlow::Fields(self.ctx.type_ctx.intern_struct(fields))
    }

    /// Logic for how quantifier on the inner expression affects the capture field.
    fn resolve_capture_inner(&mut self, inner: &Located<Pattern>) -> CaptureInner {
        if let Pattern::QuantifiedPattern(q) = inner.node() {
            let quantifier = self.quantifier_kind(q);
            match quantifier {
                // * or + acts as row capture here (skipping strict dimensionality)
                QuantifierKind::ZeroOrMore | QuantifierKind::OneOrMore => CaptureInner {
                    info: self.infer_quantified_pattern_as_row(&inner.wrap(q.clone())),
                    makes_field_optional: false,
                },
                // ? makes the resulting capture field optional
                QuantifierKind::Optional => CaptureInner {
                    info: self.infer_pattern(inner),
                    makes_field_optional: true,
                },
            }
        } else {
            CaptureInner {
                info: self.infer_pattern(inner),
                makes_field_optional: false,
            }
        }
    }

    /// The capture's base type from the inner flow, before any annotation.
    fn determine_captured_base_type(
        &mut self,
        inner: &Pattern,
        inner_info: &PatternShape,
    ) -> TypeId {
        match &inner_info.flow {
            // A truly empty scope (`{}`) captures an empty struct; any other void
            // capture is the matched node (or a recursive reference's type).
            PatternFlow::Void => {
                if is_empty_group(inner) {
                    self.ctx.type_ctx.intern_struct(BTreeMap::new())
                } else {
                    self.recursive_ref_type(inner).unwrap_or(TYPE_NODE)
                }
            }
            PatternFlow::Value(type_id) | PatternFlow::Fields(type_id) => *type_id,
        }
    }

    /// If pattern is (or contains) a recursive Ref, return its Ref type.
    fn recursive_ref_type(&mut self, pattern: &Pattern) -> Option<TypeId> {
        match pattern {
            Pattern::DefRef(r) => {
                let name_tok = r.name()?;
                let name = name_tok.text();
                let sym = self.ctx.interner.intern(name);
                let def_id = self.ctx.dependency_analysis.def_id_for_sym(sym)?;
                if self.ctx.dependency_analysis.is_recursive_def(def_id) {
                    Some(self.ctx.type_ctx.intern_type(TypeShape::Ref(def_id)))
                } else {
                    None
                }
            }
            Pattern::QuantifiedPattern(q) => self.recursive_ref_type(&q.inner()?),
            Pattern::CapturedPattern(c) => self.recursive_ref_type(&c.inner()?),
            Pattern::FieldPattern(f) => self.recursive_ref_type(&f.value()?),
            _ => None,
        }
    }

    fn infer_quantified_pattern(&mut self, quant: &Located<QuantifiedPattern>) -> PatternShape {
        self.infer_quantified_pattern_in(quant, QuantifiedCaptureMode::Bare)
    }

    fn infer_quantified_pattern_as_row(
        &mut self,
        quant: &Located<QuantifiedPattern>,
    ) -> PatternShape {
        self.infer_quantified_pattern_in(quant, QuantifiedCaptureMode::RowCapture)
    }

    fn infer_quantified_pattern_in(
        &mut self,
        quant: &Located<QuantifiedPattern>,
        capture_mode: QuantifiedCaptureMode,
    ) -> PatternShape {
        let Some(inner) = quant.node().inner() else {
            return PatternShape::void();
        };
        let inner = quant.wrap(inner);

        let inner_info = self.infer_pattern(&inner);
        let quantifier = self.quantifier_kind(quant.node());

        let flow = match quantifier {
            QuantifierKind::Optional => self.make_flow_optional(inner_info.flow),
            QuantifierKind::ZeroOrMore | QuantifierKind::OneOrMore => {
                self.check_quantified_array_dimensionality(quant.node(), &inner_info, capture_mode);
                self.make_flow_array(inner_info.flow, inner.node(), quantifier.is_non_empty())
            }
        };

        PatternShape::new(inner_info.arity, flow)
    }

    fn check_quantified_array_dimensionality(
        &mut self,
        quant: &QuantifiedPattern,
        inner_info: &PatternShape,
        capture_mode: QuantifiedCaptureMode,
    ) {
        let reported_scalar = self.report_multi_element_scalar(quant, inner_info);
        if reported_scalar {
            return;
        }

        if capture_mode == QuantifiedCaptureMode::Bare {
            self.report_internal_capture_dimensionality(quant, inner_info);
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
        inner: &Pattern,
        non_empty: bool,
    ) -> PatternFlow {
        match flow {
            PatternFlow::Void => {
                // Scalar list: void inner -> array of Node (or Ref)
                let element = self.recursive_ref_type(inner).unwrap_or(TYPE_NODE);
                let array_type = self
                    .ctx
                    .type_ctx
                    .intern_type(TypeShape::Array { element, non_empty });
                PatternFlow::Value(array_type)
            }
            PatternFlow::Value(t) => {
                let array_type = self.ctx.type_ctx.intern_type(TypeShape::Array {
                    element: t,
                    non_empty,
                });
                PatternFlow::Value(array_type)
            }
            PatternFlow::Fields(struct_type) => {
                // In `Bare` mode `report_internal_capture_dimensionality` already
                // emitted an error for this case (per-repeat output under * or +
                // without a row capture — same for structured `Value` flow above).
                // We still produce a plausible array type so downstream inference
                // isn't poisoned by void.
                let array_type = self.ctx.type_ctx.intern_type(TypeShape::Array {
                    element: struct_type,
                    non_empty,
                });
                PatternFlow::Value(array_type)
            }
        }
    }

    /// Field expression: arity One, delegates type to value.
    fn infer_field_pattern(&mut self, field: &Located<FieldPattern>) -> PatternShape {
        let Some(value) = field.node().value() else {
            return PatternShape::void();
        };
        let value = field.wrap(value);

        let value_info = self.infer_pattern(&value);

        // Validation: Fields cannot be assigned 'Many' arity values directly
        if value_info.arity == Arity::Many {
            self.report_field_arity_error(field.node(), value.node());
        }

        PatternShape::new(Arity::One, value_info.flow)
    }

    fn quantifier_kind(&self, quant: &QuantifiedPattern) -> QuantifierKind {
        // Shared with `TypeAnalysis::capture_kind` and `compile`'s implicit-array gate so the
        // three never disagree on a quantifier's arity.
        quant
            .quantifier_kind()
            .expect("quantifier kind resolved before inference")
    }
}

pub(super) struct InferPassEnv<'a, 'd> {
    pub interner: &'a mut Interner,
    pub symbol_table: &'a SymbolTable,
    pub dependency_analysis: &'a DependencyAnalysis,
    pub diag: &'d mut Diagnostics,
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

    pub fn run(mut self) -> TypeAnalysis {
        // Definition identity (names, DefIds) is owned by DependencyAnalysis and
        // read from there; the builder only accumulates inferred types.
        self.process_sccs();
        self.assert_all_definitions_processed();

        self.ctx.finish()
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
        let info = {
            let located_body = Located::new(source_id, body);
            let mut visitor = InferVisitor::new(
                InferState {
                    type_ctx: &mut self.ctx,
                    interner: self.analysis.interner,
                    symbol_table: self.analysis.symbol_table,
                    dependency_analysis: self.analysis.dependency_analysis,
                    diag: &mut *self.analysis.diag,
                },
                source_id,
            );
            visitor.infer_pattern(&located_body)
        };

        self.ctx.record_def_memo(def_id, info.clone());
        let type_id = flow_to_type(&info.flow);
        self.ctx.record_def_output(def_id, type_id);
    }
}
