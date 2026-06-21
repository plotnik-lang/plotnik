//! Bottom-up type inference visitor.
//!
//! Traverses the AST and computes TermInfo (Arity + TypeFlow) for each expression.
//! Reports diagnostics for type errors like strict dimensionality violations.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use plotnik_core::Interner;
use rowan::TextRange;

use super::capture_shape::{CaptureKind, capture_kind, quantifier_arity};
use super::context::TypeContext;
use super::def_id::Symbol;
use super::types::{
    Arity, FieldInfo, QuantifierKind, TYPE_NODE, TYPE_VOID, TermInfo, TypeFlow, TypeId, TypeShape,
};
use super::unify::{UnifyError, unify_flows};

use crate::analyze::Located;
use crate::analyze::dependencies::DependencyAnalysis;
use crate::analyze::symbol_table::SymbolTable;
use crate::diagnostics::{DiagnosticKind, Diagnostics};
use crate::parser::{
    TokenPattern, CapturedPattern, EnumPattern, Pattern, FieldPattern, NodePattern, QuantifiedPattern, UnionPattern,
    Ref, SeqPattern, is_empty_group,
};
use crate::query::SourceId;

/// Shared state for a single inference pass over the AST.
pub struct InferCtx<'a, 'd> {
    pub type_ctx: &'a mut TypeContext,
    pub interner: &'a mut Interner,
    pub symbol_table: &'a SymbolTable,
    pub(crate) diag: &'d mut Diagnostics,
}

/// Inference visitor for a single pass over the AST.
pub struct InferVisitor<'a, 'd> {
    ctx: InferCtx<'a, 'd>,
}

impl<'a, 'd> InferVisitor<'a, 'd> {
    pub fn new(ctx: InferCtx<'a, 'd>) -> Self {
        Self { ctx }
    }

    /// Infer the TermInfo for an expression, caching the result.
    ///
    /// The walk only ever descends through one definition's body (a finite AST
    /// tree); references resolve to precomputed results rather than re-entering.
    pub fn infer_pattern(&mut self, pattern: &Located<Pattern>) -> TermInfo {
        if let Some(info) = self.ctx.type_ctx.term_info(pattern.node()) {
            return info.clone();
        }

        let info = self.compute_pattern(pattern);
        self.ctx
            .type_ctx
            .cache_term_info(pattern.node().clone(), info.clone());
        info
    }

    fn compute_pattern(&mut self, pattern: &Located<Pattern>) -> TermInfo {
        match pattern.node() {
            Pattern::NodePattern(n) => self.infer_named_node(&pattern.wrap(n.clone())),
            Pattern::TokenPattern(n) => self.infer_anonymous_node(n),
            Pattern::Ref(r) => self.infer_ref(r),
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
    fn infer_named_node(&mut self, node: &Located<NodePattern>) -> TermInfo {
        let mut merged_fields: BTreeMap<Symbol, FieldInfo> = BTreeMap::new();
        let mut output_children: Vec<(TextRange, TypeId)> = Vec::new();

        for child in node.node().children() {
            let child = node.wrap(child);
            let child_info = self.infer_pattern(&child);

            match &child_info.flow {
                TypeFlow::Fields(type_id) => {
                    let fields = self.ctx.type_ctx.expect_struct_fields(*type_id).clone();
                    self.merge_fields(
                        node.source(),
                        &mut merged_fields,
                        &fields,
                        child.node().text_range(),
                    );
                }
                TypeFlow::Scalar(type_id) => {
                    if self.produces_output(*type_id) {
                        output_children.push((child.node().text_range(), *type_id));
                    }
                }
                TypeFlow::Void => {}
            }
        }

        let flow = self.compute_merged_flow(
            node.source(),
            merged_fields,
            output_children,
            node.node().text_range(),
        );
        TermInfo::new(Arity::One, flow)
    }

    /// Anonymous node (literal or wildcard): matches one position, produces nothing.
    fn infer_anonymous_node(&mut self, _node: &TokenPattern) -> TermInfo {
        TermInfo::new(Arity::One, TypeFlow::Void)
    }

    /// Reference: transparent for non-recursive defs, opaque boundary for recursive ones.
    ///
    /// A non-recursive ref resolves to its target's already-computed result rather
    /// than descending into the body. Definitions are processed in reverse-topological
    /// SCC order (leaves first), so a non-recursive target is always computed before
    /// any referrer — the body is never re-walked, and its diagnostics stay attributed
    /// to its own definition's pass (and source).
    fn infer_ref(&mut self, r: &Ref) -> TermInfo {
        let Some(name_tok) = r.name() else {
            return TermInfo::void();
        };
        let name = name_tok.text();
        let name_sym = self.ctx.interner.intern(name);

        // No definition: an undefined reference, already diagnosed upstream
        // (`UndefinedReference`). Outside the trust boundary — answer with void.
        let Some(body) = self.ctx.symbol_table.body(name) else {
            return TermInfo::void();
        };

        // Every symbol-table definition is assigned a DefId during dependency
        // analysis (each appears in exactly one SCC), so a defined ref always
        // resolves — a miss is our bug.
        let def_id = self
            .ctx
            .type_ctx
            .def_id_for_sym(name_sym)
            .expect("a defined reference has a DefId");

        // Recursive refs are opaque boundaries - they don't bubble captures.
        // For enum alternations, return Scalar(Ref) since they always produce Enum output.
        // For other definitions, return Void to avoid type errors in union alternation contexts.
        if self.ctx.type_ctx.is_recursive(def_id) {
            if self.body_is_enum(body) {
                let ref_type = self.ctx.type_ctx.intern_type(TypeShape::Ref(def_id));
                return TermInfo::new(Arity::One, TypeFlow::Scalar(ref_type));
            }
            return TermInfo::new(Arity::One, TypeFlow::Void);
        }

        // Non-recursive refs are transparent: return the target's precomputed
        // result so the enclosing scope sees its fields/arity exactly as if the
        // body were inlined here. SCC order guarantees it is already present.
        self.ctx
            .type_ctx
            .def_result(def_id)
            .cloned()
            .expect("non-recursive reference target is inferred before the referrer (SCC order)")
    }

    /// An enum body always produces an Enum type (Scalar flow), so a recursive
    /// `Ref` to such a definition is safe to treat as `Scalar(Ref)` in uncaptured
    /// contexts rather than `Void`.
    fn body_is_enum(&self, body: &Pattern) -> bool {
        matches!(body, Pattern::Enum(_))
    }

    /// Sequence: Arity aggregation, strict field merging, and output propagation.
    fn infer_seq_pattern(&mut self, seq: &Located<SeqPattern>) -> TermInfo {
        let children: Vec<Located<Pattern>> = seq.node().children().map(|c| seq.wrap(c)).collect();

        let arity = match children.len() {
            0 | 1 => children
                .first()
                .map(|c| self.infer_pattern(c).arity)
                .unwrap_or(Arity::One),
            _ => Arity::Many,
        };

        let mut merged_fields: BTreeMap<Symbol, FieldInfo> = BTreeMap::new();
        let mut output_children: Vec<(TextRange, TypeId)> = Vec::new();

        for child in &children {
            let child_info = self.infer_pattern(child);

            match &child_info.flow {
                TypeFlow::Fields(type_id) => {
                    let fields = self.ctx.type_ctx.expect_struct_fields(*type_id).clone();
                    self.merge_fields(
                        seq.source(),
                        &mut merged_fields,
                        &fields,
                        child.node().text_range(),
                    );
                }
                TypeFlow::Scalar(type_id) => {
                    if self.produces_output(*type_id) {
                        output_children.push((child.node().text_range(), *type_id));
                    }
                }
                TypeFlow::Void => {}
            }
        }

        let flow = self.compute_merged_flow(
            seq.source(),
            merged_fields,
            output_children,
            seq.node().text_range(),
        );
        TermInfo::new(arity, flow)
    }

    /// Fold `source` fields into `target` in place, reporting a diagnostic on any
    /// name collision. Shared by sequences and named nodes so both paths reject
    /// duplicate captures identically.
    fn merge_fields(
        &mut self,
        source_id: SourceId,
        target: &mut BTreeMap<Symbol, FieldInfo>,
        source: &BTreeMap<Symbol, FieldInfo>,
        range: TextRange,
    ) {
        for (&name, &info) in source {
            match target.entry(name) {
                Entry::Vacant(e) => {
                    e.insert(info);
                }
                Entry::Occupied(_) => {
                    self.ctx
                        .diag
                        .report(source_id, DiagnosticKind::DuplicateCaptureInScope, range)
                        .detail(self.ctx.interner.resolve(name))
                        .emit();
                }
            }
        }
    }

    fn infer_enum(&mut self, e: &Located<EnumPattern>) -> TermInfo {
        let mut variants: BTreeMap<Symbol, TypeId> = BTreeMap::new();
        let mut combined_arity = Arity::One;

        for branch in e.node().branches() {
            let label = branch.label().expect("enum branch must have a label");
            let label_sym = self.ctx.interner.intern(label.text());

            // A BTreeMap would silently collapse duplicate labels, leaving the enum
            // with fewer variants than the emitter expects. Reject them instead.
            if variants.contains_key(&label_sym) {
                self.ctx
                    .diag
                    .report(
                        e.source(),
                        DiagnosticKind::DuplicateAlternationLabel,
                        label.text_range(),
                    )
                    .detail(label.text())
                    .emit();
                if let Some(body) = branch.body() {
                    let body_info = self.infer_pattern(&e.wrap(body));
                    combined_arity = combined_arity.combine(body_info.arity);
                }
                continue;
            }

            let Some(body) = branch.body() else {
                // Empty variant -> Void (no payload)
                variants.insert(label_sym, TYPE_VOID);
                continue;
            };

            let body_info = self.infer_pattern(&e.wrap(body));
            combined_arity = combined_arity.combine(body_info.arity);
            variants.insert(label_sym, self.flow_to_type(&body_info.flow));
        }

        let enum_type = self.ctx.type_ctx.intern_type(TypeShape::Enum(variants));
        TermInfo::new(combined_arity, TypeFlow::Scalar(enum_type))
    }

    fn infer_union(&mut self, union: &Located<UnionPattern>) -> TermInfo {
        let mut flows: Vec<TypeFlow> = Vec::new();
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
                self.report_unify_error(union.source(), union.node().text_range(), &err);
                TypeFlow::Void
            }
        };

        TermInfo::new(combined_arity, unified_flow)
    }

    /// Captured expression: wraps inner's flow into a field.
    ///
    /// Scope creation rules:
    /// - Sequences `{...} @x` and alternations `[...] @x` create new scopes.
    ///   Inner fields become the captured type's fields.
    /// - Other expressions (named nodes, refs) don't create scopes.
    ///   Inner fields bubble up alongside the capture field.
    fn infer_captured_pattern(&mut self, cap: &Located<CapturedPattern>) -> TermInfo {
        let node = cap.node();

        // Suppressive captures don't contribute to output type
        if node.is_suppressive() {
            // Still infer inner for structural validation, but don't create fields
            return node
                .inner()
                .map(|i| self.infer_pattern(&cap.wrap(i)))
                .map(|info| TermInfo::new(info.arity, TypeFlow::Void))
                .unwrap_or_else(TermInfo::void);
        }

        let Some(name_tok) = node.name() else {
            // Recover gracefully
            return node
                .inner()
                .map(|i| self.infer_pattern(&cap.wrap(i)))
                .unwrap_or_else(TermInfo::void);
        };
        let capture_name = self.ctx.interner.intern(&name_tok.text()[1..]); // Strip @ prefix

        let annotation = self.resolve_annotation(node);

        let Some(inner) = node.inner() else {
            // Capture without inner -> a Node field (annotation may alias it).
            let type_id = annotation.map_or(TYPE_NODE, |name| self.annotate_named(TYPE_NODE, name));
            let field = FieldInfo::required(type_id);
            return TermInfo::new(
                Arity::One,
                TypeFlow::Fields(self.ctx.type_ctx.intern_single_field(capture_name, field)),
            );
        };
        let inner = cap.wrap(inner);

        // Determine how inner flow relates to capture (e.g., ? makes field optional)
        let (inner_info, is_optional) = self.resolve_capture_inner(&inner);

        // Only the `Node` mechanism captures the matched node and lets the inner's
        // fields bubble up alongside (e.g. `(named (child) @c) @cap`). Every other
        // mechanism owns the inner's fields, so they must not also bubble. Sharing
        // the classifier with emission keeps the declared type and the effects in
        // lockstep.
        let mechanism = capture_kind(inner.node(), self.ctx.type_ctx, self.ctx.interner);
        let should_merge_fields =
            mechanism == CaptureKind::Node && matches!(&inner_info.flow, TypeFlow::Fields(_));

        // The capture's base type, before its `:: …` annotation is applied.
        let base = if should_merge_fields {
            // Named node with bubbling children: the capture takes the matched node,
            // and the children bubble up alongside it.
            self.recursive_ref_type(inner.node()).unwrap_or(TYPE_NODE)
        } else {
            self.determine_captured_base_type(inner.node(), &inner_info)
        };
        let captured_type = annotation.map_or(base, |name| self.annotate_named(base, name));
        let field_info = if is_optional {
            FieldInfo::optional(captured_type)
        } else {
            FieldInfo::required(captured_type)
        };

        if should_merge_fields {
            let TypeFlow::Fields(type_id) = &inner_info.flow else {
                unreachable!()
            };
            let mut fields = self.ctx.type_ctx.expect_struct_fields(*type_id).clone();
            fields.insert(capture_name, field_info);

            TermInfo::new(
                inner_info.arity,
                TypeFlow::Fields(self.ctx.type_ctx.intern_struct(fields)),
            )
        } else {
            TermInfo::new(
                inner_info.arity,
                TypeFlow::Fields(
                    self.ctx
                        .type_ctx
                        .intern_single_field(capture_name, field_info),
                ),
            )
        }
    }

    /// `:: TypeName` — name a structured capture (struct/enum) or alias a node.
    /// Recurses into arrays and optionals so the name lands on the element.
    fn annotate_named(&mut self, type_id: TypeId, name: Symbol) -> TypeId {
        match self.ctx.type_ctx.type_shape(type_id).cloned() {
            Some(TypeShape::Struct(_) | TypeShape::Enum(_)) => {
                self.ctx.type_ctx.set_type_name(type_id, name);
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

    /// Logic for how quantifier on the inner expression affects the capture field.
    /// Returns (Info, is_optional).
    fn resolve_capture_inner(&mut self, inner: &Located<Pattern>) -> (TermInfo, bool) {
        if let Pattern::QuantifiedPattern(q) = inner.node() {
            let quantifier = self.quantifier_kind(q);
            match quantifier {
                // * or + acts as row capture here (skipping strict dimensionality)
                QuantifierKind::ZeroOrMore | QuantifierKind::OneOrMore => {
                    (self.infer_quantified_pattern_as_row(&inner.wrap(q.clone())), false)
                }
                // ? makes the resulting capture field optional
                QuantifierKind::Optional => (self.infer_pattern(inner), true),
            }
        } else {
            (self.infer_pattern(inner), false)
        }
    }

    /// The capture's base type from the inner flow, before any annotation.
    fn determine_captured_base_type(&mut self, inner: &Pattern, inner_info: &TermInfo) -> TypeId {
        match &inner_info.flow {
            // A truly empty scope (`{}`) captures an empty struct; any other void
            // capture is the matched node (or a recursive reference's type).
            TypeFlow::Void => {
                if is_empty_group(inner) {
                    self.ctx.type_ctx.intern_struct(BTreeMap::new())
                } else {
                    self.recursive_ref_type(inner).unwrap_or(TYPE_NODE)
                }
            }
            TypeFlow::Scalar(type_id) | TypeFlow::Fields(type_id) => *type_id,
        }
    }

    /// If pattern is (or contains) a recursive Ref, return its Ref type.
    fn recursive_ref_type(&mut self, pattern: &Pattern) -> Option<TypeId> {
        match pattern {
            Pattern::Ref(r) => {
                let name_tok = r.name()?;
                let name = name_tok.text();
                let sym = self.ctx.interner.intern(name);
                let def_id = self.ctx.type_ctx.def_id_for_sym(sym)?;
                if self.ctx.type_ctx.is_recursive(def_id) {
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

    fn infer_quantified_pattern(&mut self, quant: &Located<QuantifiedPattern>) -> TermInfo {
        let Some(inner) = quant.node().inner() else {
            return TermInfo::void();
        };
        let inner = quant.wrap(inner);

        let inner_info = self.infer_pattern(&inner);
        let quantifier = self.quantifier_kind(quant.node());

        let flow = match quantifier {
            QuantifierKind::Optional => self.make_flow_optional(inner_info.flow),
            QuantifierKind::ZeroOrMore | QuantifierKind::OneOrMore => {
                // A bare quantifier must satisfy both strict-dimensionality checks:
                // multi-element scalars short-circuit, otherwise internal captures
                // require a row capture this expression doesn't have.
                if !self.check_multi_element_scalar(quant.source(), quant.node(), &inner_info) {
                    self.check_internal_capture_dimensionality(
                        quant.source(),
                        quant.node(),
                        &inner_info,
                    );
                }
                self.make_flow_array(inner_info.flow, inner.node(), quantifier.is_non_empty())
            }
        };

        TermInfo::new(inner_info.arity, flow)
    }

    fn infer_quantified_pattern_as_row(&mut self, quant: &Located<QuantifiedPattern>) -> TermInfo {
        let Some(inner) = quant.node().inner() else {
            return TermInfo::void();
        };
        let inner = quant.wrap(inner);

        let inner_info = self.infer_pattern(&inner);
        let quantifier = self.quantifier_kind(quant.node());

        let flow = match quantifier {
            QuantifierKind::Optional => self.make_flow_optional(inner_info.flow),
            QuantifierKind::ZeroOrMore | QuantifierKind::OneOrMore => {
                // The surrounding row capture supplies the missing dimension, so only
                // the multi-element scalar check still applies; internal captures are
                // legal here.
                self.check_multi_element_scalar(quant.source(), quant.node(), &inner_info);
                self.make_flow_array(inner_info.flow, inner.node(), quantifier.is_non_empty())
            }
        };

        TermInfo::new(inner_info.arity, flow)
    }

    fn make_flow_optional(&mut self, flow: TypeFlow) -> TypeFlow {
        match flow {
            TypeFlow::Void => TypeFlow::Void,
            TypeFlow::Scalar(t) => {
                TypeFlow::Scalar(self.ctx.type_ctx.intern_type(TypeShape::Optional(t)))
            }
            TypeFlow::Fields(type_id) => {
                let fields = self.ctx.type_ctx.expect_struct_fields(type_id).clone();
                let optional_fields = fields
                    .into_iter()
                    .map(|(k, v)| (k, v.make_optional()))
                    .collect();
                TypeFlow::Fields(self.ctx.type_ctx.intern_struct(optional_fields))
            }
        }
    }

    fn make_flow_array(&mut self, flow: TypeFlow, inner: &Pattern, non_empty: bool) -> TypeFlow {
        match flow {
            TypeFlow::Void => {
                // Scalar list: void inner -> array of Node (or Ref)
                let element = self.recursive_ref_type(inner).unwrap_or(TYPE_NODE);
                let array_type = self
                    .ctx
                    .type_ctx
                    .intern_type(TypeShape::Array { element, non_empty });
                TypeFlow::Scalar(array_type)
            }
            TypeFlow::Scalar(t) => {
                let array_type = self.ctx.type_ctx.intern_type(TypeShape::Array {
                    element: t,
                    non_empty,
                });
                TypeFlow::Scalar(array_type)
            }
            TypeFlow::Fields(struct_type) => {
                // `check_internal_capture_dimensionality` already emitted an error for
                // this case (Fields under * or + without a row capture). We still
                // produce a plausible array type so downstream inference isn't poisoned
                // by void.
                let array_type = self.ctx.type_ctx.intern_type(TypeShape::Array {
                    element: struct_type,
                    non_empty,
                });
                TypeFlow::Scalar(array_type)
            }
        }
    }

    /// Field expression: arity One, delegates type to value.
    fn infer_field_pattern(&mut self, field: &Located<FieldPattern>) -> TermInfo {
        let Some(value) = field.node().value() else {
            return TermInfo::void();
        };
        let value = field.wrap(value);

        let value_info = self.infer_pattern(&value);

        // Validation: Fields cannot be assigned 'Many' arity values directly
        if value_info.arity == Arity::Many {
            self.report_field_arity_error(field.source(), field.node(), value.node());
        }

        TermInfo::new(Arity::One, value_info.flow)
    }

    fn report_field_arity_error(&mut self, source: SourceId, field: &FieldPattern, value: &Pattern) {
        let field_name = field
            .name()
            .map(|t| t.text().to_string())
            .unwrap_or_else(|| "field".to_string());

        let mut builder = self
            .ctx
            .diag
            .report(source, DiagnosticKind::FieldSequenceValue, value.text_range());
        builder = builder.detail(field_name);

        if let Pattern::Ref(r) = value
            && let Some(name_tok) = r.name()
        {
            let name = name_tok.text();
            if let Some((src, body)) = self.ctx.symbol_table.definition(name) {
                builder = builder.related_to(src, body.text_range(), "defined here");
            }
        }

        builder.emit();
    }

    /// Strict-dimensionality check 1: a multi-element pattern (`Arity::Many`)
    /// without captures can't become a scalar array. Applies even under a row
    /// capture — you can't meaningfully capture multiple nodes per iteration as
    /// a scalar. Returns `true` when it reports, signalling the caller to skip
    /// the internal-capture check (the original short-circuit).
    fn check_multi_element_scalar(
        &mut self,
        source: SourceId,
        quant: &QuantifiedPattern,
        inner_info: &TermInfo,
    ) -> bool {
        if !(inner_info.arity == Arity::Many && inner_info.flow.is_void()) {
            return false;
        }

        let op = self.quantifier_operator(quant);
        self.ctx
            .diag
            .report(
                source,
                DiagnosticKind::MultiElementScalarCapture,
                quant.text_range(),
            )
            .detail(format!(
                "sequence with `{}` matches multiple nodes but has no internal captures",
                op
            ))
            .emit();
        true
    }

    /// Strict-dimensionality check 2: internal captures require a row capture on
    /// the quantifier. Skipped when the quantifier already sits under a row
    /// capture (see `infer_quantified_pattern_as_row`).
    fn check_internal_capture_dimensionality(
        &mut self,
        source: SourceId,
        quant: &QuantifiedPattern,
        inner_info: &TermInfo,
    ) {
        let TypeFlow::Fields(type_id) = &inner_info.flow else {
            return;
        };

        let fields = self.ctx.type_ctx.expect_struct_fields(*type_id);
        if fields.is_empty() {
            return;
        }

        let capture_names: Vec<_> = fields
            .keys()
            .map(|s| format!("`@{}`", self.ctx.interner.resolve(*s)))
            .collect();
        let captures_str = capture_names.join(", ");

        let op = self.quantifier_operator(quant);
        self.ctx
            .diag
            .report(
                source,
                DiagnosticKind::StrictDimensionalityViolation,
                quant.text_range(),
            )
            .detail(format!(
                "quantifier `{}` contains captures ({}) but has no struct capture",
                op, captures_str
            ))
            .hint(format!("add a struct capture: `{{...}}{} @name`", op))
            .emit();
    }

    fn quantifier_operator(&self, quant: &QuantifiedPattern) -> String {
        quant
            .operator()
            .map(|t| t.text().to_string())
            .unwrap_or_else(|| "*".to_string())
    }

    fn quantifier_kind(&self, quant: &QuantifiedPattern) -> QuantifierKind {
        // Shared with `capture_kind` and `compile`'s implicit-array gate so the
        // three never disagree on a quantifier's arity. A malformed operator-less
        // quantifier can't reach inference, so the fallback is unreachable in practice.
        quantifier_arity(quant).unwrap_or(QuantifierKind::ZeroOrMore)
    }

    fn flow_to_type(&mut self, flow: &TypeFlow) -> TypeId {
        match flow {
            TypeFlow::Void => TYPE_VOID,
            TypeFlow::Scalar(t) | TypeFlow::Fields(t) => *t,
        }
    }

    /// Check if a type produces meaningful output for propagation.
    ///
    /// Meaningful outputs are structured types (enums, structs, refs) or arrays/optionals
    /// of such types. Simple `Node[]` from quantified named nodes is NOT meaningful.
    fn produces_output(&self, type_id: TypeId) -> bool {
        let Some(shape) = self.ctx.type_ctx.type_shape(type_id) else {
            return false;
        };
        match shape {
            TypeShape::Enum(_) | TypeShape::Struct(_) | TypeShape::Ref(_) => true,
            TypeShape::Array { element, .. } => {
                *element != TYPE_NODE && self.produces_output(*element)
            }
            TypeShape::Optional(inner) => *inner != TYPE_NODE && self.produces_output(*inner),
            TypeShape::Node | TypeShape::Void | TypeShape::Custom(_) => false,
        }
    }

    /// Compute flow from merged bubble fields and output-producing children.
    ///
    /// Rules:
    /// - No bubbles, 0 outputs → Void
    /// - No bubbles, 1 output → Forward output (propagate)
    /// - No bubbles, 2+ outputs → Error (ambiguous)
    /// - Bubbles, 0 outputs → Fields(struct)
    /// - Bubbles, 1+ outputs → Error (require capture)
    fn compute_merged_flow(
        &mut self,
        source: SourceId,
        merged_fields: BTreeMap<Symbol, FieldInfo>,
        output_children: Vec<(TextRange, TypeId)>,
        parent_range: TextRange,
    ) -> TypeFlow {
        let has_bubbles = !merged_fields.is_empty();

        match (has_bubbles, output_children.len()) {
            (false, 0) => TypeFlow::Void,
            (false, 1) => TypeFlow::Scalar(output_children[0].1),
            (false, _) => {
                self.report_ambiguous_outputs(source, parent_range, &output_children);
                TypeFlow::Void
            }
            (true, 0) => TypeFlow::Fields(self.ctx.type_ctx.intern_struct(merged_fields)),
            (true, _) => {
                self.report_uncaptured_output_with_captures(source, &output_children);
                TypeFlow::Fields(self.ctx.type_ctx.intern_struct(merged_fields))
            }
        }
    }

    fn report_ambiguous_outputs(
        &mut self,
        source: SourceId,
        parent_range: TextRange,
        outputs: &[(TextRange, TypeId)],
    ) {
        let mut builder = self
            .ctx
            .diag
            .report(source, DiagnosticKind::AmbiguousUncapturedOutputs, parent_range)
            .detail(format!(
                "{} expressions here produce a value but none is captured",
                outputs.len()
            ));
        for (range, _) in outputs {
            builder = builder.related_to(source, *range, "produces a value");
        }
        builder.emit();
    }

    fn report_uncaptured_output_with_captures(
        &mut self,
        source: SourceId,
        outputs: &[(TextRange, TypeId)],
    ) {
        for (range, _) in outputs {
            self.ctx
                .diag
                .report(source, DiagnosticKind::UncapturedOutputWithCaptures, *range)
                .emit();
        }
    }

    fn report_unify_error(&mut self, source: SourceId, range: TextRange, err: &UnifyError) {
        let (kind, msg, hint) = match err {
            UnifyError::ScalarInUnion => (
                DiagnosticKind::IncompatibleTypes,
                "a branch produces a value but the alternation is unlabeled".to_string(),
                Some("give every branch a branch label for an enum, e.g. `[A: ... B: ...]`"),
            ),
            UnifyError::IncompatibleTypes { field } => (
                DiagnosticKind::IncompatibleCaptureTypes,
                self.ctx.interner.resolve(*field).to_string(),
                Some(
                    "make every branch produce the same type, or label the branches for an enum",
                ),
            ),
            UnifyError::IncompatibleStructs { field } => (
                DiagnosticKind::IncompatibleStructShapes,
                self.ctx.interner.resolve(*field).to_string(),
                Some("use an enum if branches need different fields"),
            ),
            UnifyError::IncompatibleArrayElements { field } => (
                DiagnosticKind::IncompatibleCaptureTypes,
                self.ctx.interner.resolve(*field).to_string(),
                Some("array element types must be compatible across branches"),
            ),
        };

        let mut builder = self.ctx.diag.report(source, kind, range).detail(msg);
        if let Some(h) = hint {
            builder = builder.hint(h);
        }
        builder.emit();
    }
}

pub(super) struct InferPassInput<'a, 'd> {
    pub interner: &'a mut Interner,
    pub symbol_table: &'a SymbolTable,
    pub dependency_analysis: &'a DependencyAnalysis,
    pub diag: &'d mut Diagnostics,
}

/// Orchestrates type inference across all definitions in dependency order.
pub(super) struct InferencePass<'a, 'd> {
    ctx: TypeContext,
    analysis: InferPassInput<'a, 'd>,
}

impl<'a, 'd> InferencePass<'a, 'd> {
    pub fn new(analysis: InferPassInput<'a, 'd>) -> Self {
        Self {
            ctx: TypeContext::new(),
            analysis,
        }
    }

    pub fn run(mut self) -> TypeContext {
        // Avoid re-registration of definitions
        self.ctx.seed_defs(
            self.analysis.dependency_analysis.def_names(),
            self.analysis.dependency_analysis.def_ids_by_sym(),
        );

        self.mark_recursion();
        self.process_sccs();
        self.process_orphans();

        self.ctx
    }

    /// Identify and mark recursive definitions.
    fn mark_recursion(&mut self) {
        for scc in self.analysis.dependency_analysis.sccs() {
            for def_name in scc {
                if !self.analysis.dependency_analysis.is_recursive(def_name) {
                    continue;
                }
                let sym = self.analysis.interner.intern(def_name);
                let Some(def_id) = self.ctx.def_id_for_sym(sym) else {
                    continue;
                };
                self.ctx.mark_recursive(def_id);
            }
        }
    }

    /// Process definitions in SCC order (leaves first).
    fn process_sccs(&mut self) {
        for scc in self.analysis.dependency_analysis.sccs() {
            for def_name in scc {
                if let Some(source_id) = self.analysis.symbol_table.source_id(def_name) {
                    self.infer_and_register(def_name, source_id);
                }
            }
        }
    }

    /// Handle any definitions not in an SCC (safety net).
    fn process_orphans(&mut self) {
        for (name, source_id, _body) in self.analysis.symbol_table.definitions() {
            if self
                .ctx
                .def_type_for_name(self.analysis.interner, name)
                .is_some()
            {
                continue;
            }
            self.infer_and_register(name, source_id);
        }
    }

    fn infer_and_register(&mut self, def_name: &str, source_id: SourceId) {
        let Some(body) = self.analysis.symbol_table.body(def_name).cloned() else {
            return;
        };

        // Infer this definition's body only; references into other definitions
        // resolve to their precomputed results.
        let info = {
            let located_body = Located::new(source_id, body);
            let mut visitor = InferVisitor::new(InferCtx {
                type_ctx: &mut self.ctx,
                interner: self.analysis.interner,
                symbol_table: self.analysis.symbol_table,
                diag: &mut *self.analysis.diag,
            });
            visitor.infer_pattern(&located_body)
        };

        let def_id = self.ctx.register_def(self.analysis.interner, def_name);
        self.ctx.set_def_result(def_id, info.clone());
        let type_id = self.flow_to_type_id(&info.flow);
        self.ctx.set_def_type(def_id, type_id);
    }

    fn flow_to_type_id(&mut self, flow: &TypeFlow) -> TypeId {
        match flow {
            TypeFlow::Void => TYPE_VOID,
            TypeFlow::Scalar(id) | TypeFlow::Fields(id) => *id,
        }
    }
}
