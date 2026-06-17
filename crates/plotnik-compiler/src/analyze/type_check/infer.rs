//! Bottom-up type inference visitor.
//!
//! Traverses the AST and computes TermInfo (Arity + TypeFlow) for each expression.
//! Reports diagnostics for type errors like strict dimensionality violations.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use indexmap::IndexMap;
use plotnik_core::Interner;
use rowan::TextRange;

use super::capture_shape::{CaptureMechanism, capture_mechanism, quantifier_arity};
use super::context::TypeContext;
use super::symbol::Symbol;
use super::types::{
    Arity, FieldInfo, QuantifierKind, TYPE_NODE, TYPE_STRING, TYPE_VOID, TermInfo, TypeFlow,
    TypeId, TypeShape,
};
use super::unify::{UnifyError, unify_flows};

use crate::analyze::Reporter;
use crate::analyze::dependencies::DependencyAnalysis;
use crate::analyze::symbol_table::SymbolTable;
use crate::analyze::visitor::{Visitor, walk_alt_expr, walk_def, walk_named_node, walk_seq_expr};
use crate::diagnostics::{DiagnosticKind, Diagnostics};
use crate::parser::{
    AltExpr, AltKind, AnonymousNode, CapturedExpr, Def, Expr, FieldExpr, NamedNode, QuantifiedExpr,
    Ref, Root, SeqExpr, is_truly_empty_scope,
};
use crate::query::SourceId;

/// Type annotation kind from `@capture :: Type` syntax.
///
/// The caller decides how to use the annotation based on context:
/// - `String`: always converts the capture to string type
/// - `TypeName`: either names a struct (for scope-creating captures) or creates a Node alias
#[derive(Clone, Copy, Debug)]
enum AnnotationKind {
    /// `:: string` - extract text as string
    String,
    /// `:: TypeName` - custom type name
    TypeName(Symbol),
}

/// Shared state for a single inference pass over the AST.
pub struct InferenceContext<'a, 'd> {
    pub type_ctx: &'a mut TypeContext,
    pub interner: &'a mut Interner,
    pub symbol_table: &'a SymbolTable,
    pub(crate) reporter: Reporter<'d>,
}

/// Inference visitor for a single pass over the AST.
pub struct InferenceVisitor<'a, 'd> {
    ctx: InferenceContext<'a, 'd>,
}

impl<'a, 'd> InferenceVisitor<'a, 'd> {
    pub fn new(ctx: InferenceContext<'a, 'd>) -> Self {
        Self { ctx }
    }

    /// Switch the active source for the cross-file descent in `f`, so the
    /// diagnostics it emits resolve against the referenced file's content.
    fn with_source<R>(&mut self, source: SourceId, f: impl FnOnce(&mut Self) -> R) -> R {
        let saved = self.ctx.reporter.swap_source(source);
        let result = f(self);
        self.ctx.reporter.swap_source(saved);
        result
    }

    /// Infer the TermInfo for an expression, caching the result.
    pub fn infer_expr(&mut self, expr: &Expr) -> TermInfo {
        if let Some(info) = self.ctx.type_ctx.get_term_info(expr) {
            return info.clone();
        }

        // Sentinel to break recursion cycles
        self.ctx
            .type_ctx
            .set_term_info(expr.clone(), TermInfo::void());

        let info = self.compute_expr(expr);
        self.ctx.type_ctx.set_term_info(expr.clone(), info.clone());
        info
    }

    fn compute_expr(&mut self, expr: &Expr) -> TermInfo {
        match expr {
            Expr::NamedNode(n) => self.infer_named_node(n),
            Expr::AnonymousNode(n) => self.infer_anonymous_node(n),
            Expr::Ref(r) => self.infer_ref(r),
            Expr::SeqExpr(s) => self.infer_seq_expr(s),
            Expr::AltExpr(a) => self.infer_alt_expr(a),
            Expr::CapturedExpr(c) => self.infer_captured_expr(c),
            Expr::QuantifiedExpr(q) => self.infer_quantified_expr(q),
            Expr::FieldExpr(f) => self.infer_field_expr(f),
        }
    }

    /// Named node: matches one position, bubbles up child captures or propagates output.
    fn infer_named_node(&mut self, node: &NamedNode) -> TermInfo {
        let mut merged_fields: BTreeMap<Symbol, FieldInfo> = BTreeMap::new();
        let mut output_children: Vec<(TextRange, TypeId)> = Vec::new();

        for child in node.children() {
            let child_info = self.infer_expr(&child);

            match &child_info.flow {
                TypeFlow::Bubble(type_id) => {
                    let fields = self.ctx.type_ctx.expect_struct_fields(*type_id).clone();
                    self.merge_fields(&mut merged_fields, &fields, child.text_range());
                }
                TypeFlow::Scalar(type_id) => {
                    if self.produces_output(*type_id) {
                        output_children.push((child.text_range(), *type_id));
                    }
                }
                TypeFlow::Void => {}
            }
        }

        let flow = self.compute_merged_flow(merged_fields, output_children, node.text_range());
        TermInfo::new(Arity::One, flow)
    }

    /// Anonymous node (literal or wildcard): matches one position, produces nothing.
    fn infer_anonymous_node(&mut self, _node: &AnonymousNode) -> TermInfo {
        TermInfo::new(Arity::One, TypeFlow::Void)
    }

    /// Reference: transparent for non-recursive defs, opaque boundary for recursive ones.
    fn infer_ref(&mut self, r: &Ref) -> TermInfo {
        let Some(name_tok) = r.name() else {
            return TermInfo::void();
        };
        let name = name_tok.text();
        let name_sym = self.ctx.interner.intern(name);

        let Some((ref_source, body)) = self.ctx.symbol_table.get_full(name) else {
            return TermInfo::void();
        };

        // Recursive refs are opaque boundaries - they don't bubble captures.
        // For tagged alternations, return Scalar(Ref) since they always produce Enum output.
        // For other definitions, return Void to avoid type errors in untagged alternation contexts.
        if let Some(def_id) = self.ctx.type_ctx.get_def_id_sym(name_sym)
            && self.ctx.type_ctx.is_recursive(def_id)
        {
            if self.body_produces_enum(body) {
                let ref_type = self.ctx.type_ctx.intern_type(TypeShape::Ref(def_id));
                return TermInfo::new(Arity::One, TypeFlow::Scalar(ref_type));
            }
            return TermInfo::new(Arity::One, TypeFlow::Void);
        }

        // Non-recursive refs are transparent. The body may live in another
        // workspace file, so expand it under its own source — otherwise any
        // diagnostic emitted here carries this file's source id with a foreign
        // text range (out-of-bounds when slicing the wrong content).
        self.with_source(ref_source, |this| this.infer_expr(body))
    }

    /// Check if an expression body will produce an Enum type (Scalar flow).
    ///
    /// This is a syntactic check for tagged alternations at the root of a definition.
    /// Tagged alternations always produce Enum types, making them safe to reference
    /// as Scalar(Ref) in uncaptured contexts.
    fn body_produces_enum(&self, body: &Expr) -> bool {
        if let Expr::AltExpr(alt) = body {
            matches!(alt.kind(), AltKind::Tagged | AltKind::Mixed)
        } else {
            false
        }
    }

    /// Sequence: Arity aggregation, strict field merging, and output propagation.
    fn infer_seq_expr(&mut self, seq: &SeqExpr) -> TermInfo {
        let children: Vec<_> = seq.children().collect();

        let arity = match children.len() {
            0 | 1 => children
                .first()
                .map(|c| self.infer_expr(c).arity)
                .unwrap_or(Arity::One),
            _ => Arity::Many,
        };

        let mut merged_fields: BTreeMap<Symbol, FieldInfo> = BTreeMap::new();
        let mut output_children: Vec<(TextRange, TypeId)> = Vec::new();

        for child in &children {
            let child_info = self.infer_expr(child);

            match &child_info.flow {
                TypeFlow::Bubble(type_id) => {
                    let fields = self.ctx.type_ctx.expect_struct_fields(*type_id).clone();
                    self.merge_fields(&mut merged_fields, &fields, child.text_range());
                }
                TypeFlow::Scalar(type_id) => {
                    if self.produces_output(*type_id) {
                        output_children.push((child.text_range(), *type_id));
                    }
                }
                TypeFlow::Void => {}
            }
        }

        let flow = self.compute_merged_flow(merged_fields, output_children, seq.text_range());
        TermInfo::new(arity, flow)
    }

    /// Merge `source` fields into `target`, reporting a diagnostic on any name
    /// collision. Shared by sequences and named nodes so both paths reject
    /// duplicate captures identically.
    fn merge_fields(
        &mut self,
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
                        .reporter
                        .report(DiagnosticKind::DuplicateCaptureInScope, range)
                        .message(self.ctx.interner.resolve(name))
                        .emit();
                }
            }
        }
    }

    /// Alternation: arity is Many if ANY branch is Many.
    fn infer_alt_expr(&mut self, alt: &AltExpr) -> TermInfo {
        match alt.kind() {
            AltKind::Tagged => self.infer_tagged_alt(alt),
            AltKind::Untagged | AltKind::Mixed => self.infer_untagged_alt(alt),
        }
    }

    fn infer_tagged_alt(&mut self, alt: &AltExpr) -> TermInfo {
        let mut variants: BTreeMap<Symbol, TypeId> = BTreeMap::new();
        let mut combined_arity = Arity::One;

        for branch in alt.branches() {
            let Some(label) = branch.label() else {
                continue;
            };
            let label_sym = self.ctx.interner.intern(label.text());

            // A BTreeMap would silently collapse duplicate labels, leaving the enum
            // with fewer variants than the emitter expects. Reject them instead.
            if variants.contains_key(&label_sym) {
                self.ctx
                    .reporter
                    .report(
                        DiagnosticKind::DuplicateAlternationLabel,
                        label.text_range(),
                    )
                    .message(label.text())
                    .emit();
                if let Some(body) = branch.body() {
                    let body_info = self.infer_expr(&body);
                    combined_arity = combined_arity.combine(body_info.arity);
                }
                continue;
            }

            let Some(body) = branch.body() else {
                // Empty variant -> Void (no payload)
                variants.insert(label_sym, TYPE_VOID);
                continue;
            };

            let body_info = self.infer_expr(&body);
            combined_arity = combined_arity.combine(body_info.arity);
            variants.insert(label_sym, self.flow_to_type(&body_info.flow));
        }

        let enum_type = self.ctx.type_ctx.intern_type(TypeShape::Enum(variants));
        TermInfo::new(combined_arity, TypeFlow::Scalar(enum_type))
    }

    fn infer_untagged_alt(&mut self, alt: &AltExpr) -> TermInfo {
        let mut flows: Vec<TypeFlow> = Vec::new();
        let mut combined_arity = Arity::One;

        for branch in alt.branches() {
            if let Some(body) = branch.body() {
                let info = self.infer_expr(&body);
                combined_arity = combined_arity.combine(info.arity);
                flows.push(info.flow);
            }
        }

        for expr in alt.exprs() {
            let info = self.infer_expr(&expr);
            combined_arity = combined_arity.combine(info.arity);
            flows.push(info.flow);
        }

        let unified_flow = match unify_flows(self.ctx.type_ctx, flows) {
            Ok(flow) => flow,
            Err(err) => {
                self.report_unify_error(alt.text_range(), &err);
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
    fn infer_captured_expr(&mut self, cap: &CapturedExpr) -> TermInfo {
        // Suppressive captures don't contribute to output type
        if cap.is_suppressive() {
            // Still infer inner for structural validation, but don't create fields
            return cap
                .inner()
                .map(|i| self.infer_expr(&i))
                .map(|info| TermInfo::new(info.arity, TypeFlow::Void))
                .unwrap_or_else(TermInfo::void);
        }

        let Some(name_tok) = cap.name() else {
            // Recover gracefully
            return cap
                .inner()
                .map(|i| self.infer_expr(&i))
                .unwrap_or_else(TermInfo::void);
        };
        let capture_name = self.ctx.interner.intern(&name_tok.text()[1..]); // Strip @ prefix

        let annotation = self.resolve_annotation(cap);
        let ann_range = cap
            .type_annotation()
            .map(|t| t.text_range())
            .unwrap_or_else(|| name_tok.text_range());

        let Some(inner) = cap.inner() else {
            // Capture without inner -> a Node field (annotation may alias/stringify it).
            let type_id = self.apply_annotation(TYPE_NODE, annotation, ann_range);
            let field = FieldInfo::required(type_id);
            return TermInfo::new(
                Arity::One,
                TypeFlow::Bubble(self.ctx.type_ctx.intern_single_field(capture_name, field)),
            );
        };

        // Determine how inner flow relates to capture (e.g., ? makes field optional)
        let (inner_info, is_optional) = self.resolve_capture_inner(&inner);

        // Only the `Node` mechanism captures the matched node and lets the inner's
        // fields bubble up alongside (e.g. `(named (child) @c) @cap`). Every other
        // mechanism owns the inner's fields, so they must not also bubble. Sharing
        // the classifier with emission keeps the declared type and the effects in
        // lockstep.
        let mechanism = capture_mechanism(&inner, self.ctx.type_ctx, self.ctx.interner);
        let should_merge_fields =
            mechanism == CaptureMechanism::Node && matches!(&inner_info.flow, TypeFlow::Bubble(_));

        // The capture's base type, before its `:: …` annotation is applied.
        let base = if should_merge_fields {
            // Named node with bubbling children: the capture takes the matched node,
            // and the children bubble up alongside it.
            self.get_recursive_ref_type(&inner).unwrap_or(TYPE_NODE)
        } else {
            self.determine_captured_base_type(&inner, &inner_info)
        };
        let captured_type = self.apply_annotation(base, annotation, ann_range);
        let field_info = if is_optional {
            FieldInfo::optional(captured_type)
        } else {
            FieldInfo::required(captured_type)
        };

        if should_merge_fields {
            let TypeFlow::Bubble(type_id) = &inner_info.flow else {
                unreachable!()
            };
            let mut fields = self.ctx.type_ctx.expect_struct_fields(*type_id).clone();
            fields.insert(capture_name, field_info);

            TermInfo::new(
                inner_info.arity,
                TypeFlow::Bubble(self.ctx.type_ctx.intern_struct(fields)),
            )
        } else {
            TermInfo::new(
                inner_info.arity,
                TypeFlow::Bubble(
                    self.ctx
                        .type_ctx
                        .intern_single_field(capture_name, field_info),
                ),
            )
        }
    }

    /// Apply a `:: string` / `:: TypeName` annotation to a capture's base type.
    ///
    /// The single place that decides what each annotation means for each shape —
    /// recursing through arrays and optionals so the annotation lands on the
    /// element, and rejecting combinations that have no meaning.
    fn apply_annotation(
        &mut self,
        base: TypeId,
        annotation: Option<AnnotationKind>,
        range: TextRange,
    ) -> TypeId {
        match annotation {
            None => base,
            Some(AnnotationKind::String) => self.annotate_string(base, range),
            Some(AnnotationKind::TypeName(name)) => self.annotate_named(base, name),
        }
    }

    /// `:: string` — project the matched node's text. Recurses into arrays and
    /// optionals; structured captures (struct/enum) have no text form and are
    /// rejected with a diagnostic.
    fn annotate_string(&mut self, type_id: TypeId, range: TextRange) -> TypeId {
        match self.ctx.type_ctx.get_type(type_id).cloned() {
            Some(TypeShape::Node | TypeShape::String | TypeShape::Custom(_)) => TYPE_STRING,
            Some(TypeShape::Array { element, non_empty }) => {
                let element = self.annotate_string(element, range);
                self.ctx
                    .type_ctx
                    .intern_type(TypeShape::Array { element, non_empty })
            }
            Some(TypeShape::Optional(inner)) => {
                let inner = self.annotate_string(inner, range);
                self.ctx.type_ctx.intern_type(TypeShape::Optional(inner))
            }
            _ => {
                self.report_invalid_annotation(
                    range,
                    "`:: string` cannot extract text from a structured capture",
                );
                type_id
            }
        }
    }

    /// `:: TypeName` — name a structured capture (struct/enum) or alias a node.
    /// Recurses into arrays and optionals so the name lands on the element.
    fn annotate_named(&mut self, type_id: TypeId, name: Symbol) -> TypeId {
        match self.ctx.type_ctx.get_type(type_id).cloned() {
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
            // Node, recursive Ref, string, or void: a named alias to the value.
            _ => self.ctx.type_ctx.intern_type(TypeShape::Custom(name)),
        }
    }

    fn report_invalid_annotation(&mut self, range: TextRange, message: &str) {
        self.ctx
            .reporter
            .report(DiagnosticKind::InvalidTypeAnnotation, range)
            .message(message)
            .emit();
    }

    /// Resolves explicit type annotation like `@foo :: string` or `@foo :: TypeName`.
    ///
    /// Returns the annotation kind without creating types - the caller decides
    /// how to use the annotation based on the capture's flow.
    fn resolve_annotation(&mut self, cap: &CapturedExpr) -> Option<AnnotationKind> {
        cap.type_annotation().and_then(|t| {
            t.name().map(|n| {
                let text = n.text();
                if text == "string" {
                    AnnotationKind::String
                } else {
                    AnnotationKind::TypeName(self.ctx.interner.intern(text))
                }
            })
        })
    }

    /// Logic for how quantifier on the inner expression affects the capture field.
    /// Returns (Info, is_optional).
    fn resolve_capture_inner(&mut self, inner: &Expr) -> (TermInfo, bool) {
        if let Expr::QuantifiedExpr(q) = inner {
            let quantifier = self.parse_quantifier(q);
            match quantifier {
                // * or + acts as row capture here (skipping strict dimensionality)
                QuantifierKind::ZeroOrMore | QuantifierKind::OneOrMore => {
                    (self.infer_quantified_expr_as_row(q), false)
                }
                // ? makes the resulting capture field optional
                QuantifierKind::Optional => (self.infer_expr(inner), true),
            }
        } else {
            (self.infer_expr(inner), false)
        }
    }

    /// The capture's base type from the inner flow, before any annotation.
    fn determine_captured_base_type(&mut self, inner: &Expr, inner_info: &TermInfo) -> TypeId {
        match &inner_info.flow {
            // A truly empty scope (`{}`) captures an empty struct; any other void
            // capture is the matched node (or a recursive reference's type).
            TypeFlow::Void => {
                if is_truly_empty_scope(inner) {
                    self.ctx.type_ctx.intern_struct(BTreeMap::new())
                } else {
                    self.get_recursive_ref_type(inner).unwrap_or(TYPE_NODE)
                }
            }
            TypeFlow::Scalar(type_id) | TypeFlow::Bubble(type_id) => *type_id,
        }
    }

    /// If expr is (or contains) a recursive Ref, return its Ref type.
    fn get_recursive_ref_type(&mut self, expr: &Expr) -> Option<TypeId> {
        match expr {
            Expr::Ref(r) => {
                let name_tok = r.name()?;
                let name = name_tok.text();
                let sym = self.ctx.interner.intern(name);
                let def_id = self.ctx.type_ctx.get_def_id_sym(sym)?;
                if self.ctx.type_ctx.is_recursive(def_id) {
                    Some(self.ctx.type_ctx.intern_type(TypeShape::Ref(def_id)))
                } else {
                    None
                }
            }
            Expr::QuantifiedExpr(q) => self.get_recursive_ref_type(&q.inner()?),
            Expr::CapturedExpr(c) => self.get_recursive_ref_type(&c.inner()?),
            Expr::FieldExpr(f) => self.get_recursive_ref_type(&f.value()?),
            _ => None,
        }
    }

    fn infer_quantified_expr(&mut self, quant: &QuantifiedExpr) -> TermInfo {
        self.infer_quantified_expr_impl(quant, false)
    }

    fn infer_quantified_expr_as_row(&mut self, quant: &QuantifiedExpr) -> TermInfo {
        self.infer_quantified_expr_impl(quant, true)
    }

    fn infer_quantified_expr_impl(
        &mut self,
        quant: &QuantifiedExpr,
        is_row_capture: bool,
    ) -> TermInfo {
        let Some(inner) = quant.inner() else {
            return TermInfo::void();
        };

        let inner_info = self.infer_expr(&inner);
        let quantifier = self.parse_quantifier(quant);

        let flow = match quantifier {
            QuantifierKind::Optional => self.make_flow_optional(inner_info.flow),
            QuantifierKind::ZeroOrMore | QuantifierKind::OneOrMore => {
                // Always check multi-element sequences (row capture doesn't help)
                // Only skip internal capture check when is_row_capture
                self.check_strict_dimensionality(quant, &inner_info, is_row_capture);
                self.make_flow_array(inner_info.flow, &inner, quantifier.is_non_empty())
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
            TypeFlow::Bubble(type_id) => {
                let fields = self.ctx.type_ctx.expect_struct_fields(type_id).clone();
                let optional_fields = fields
                    .into_iter()
                    .map(|(k, v)| (k, v.make_optional()))
                    .collect();
                TypeFlow::Bubble(self.ctx.type_ctx.intern_struct(optional_fields))
            }
        }
    }

    fn make_flow_array(&mut self, flow: TypeFlow, inner: &Expr, non_empty: bool) -> TypeFlow {
        match flow {
            TypeFlow::Void => {
                // Scalar list: void inner -> array of Node (or Ref)
                let element = self.get_recursive_ref_type(inner).unwrap_or(TYPE_NODE);
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
            TypeFlow::Bubble(struct_type) => {
                // `check_strict_dimensionality` already emitted an error for this case
                // (Bubble under * or + without a row capture). We still produce a
                // plausible array type so downstream inference isn't poisoned by void.
                let array_type = self.ctx.type_ctx.intern_type(TypeShape::Array {
                    element: struct_type,
                    non_empty,
                });
                TypeFlow::Scalar(array_type)
            }
        }
    }

    /// Field expression: arity One, delegates type to value.
    fn infer_field_expr(&mut self, field: &FieldExpr) -> TermInfo {
        let Some(value) = field.value() else {
            return TermInfo::void();
        };

        let value_info = self.infer_expr(&value);

        // Validation: Fields cannot be assigned 'Many' arity values directly
        if value_info.arity == Arity::Many {
            self.report_field_arity_error(field, &value);
        }

        TermInfo::new(Arity::One, value_info.flow)
    }

    fn report_field_arity_error(&mut self, field: &FieldExpr, value: &Expr) {
        let field_name = field
            .name()
            .map(|t| t.text().to_string())
            .unwrap_or_else(|| "field".to_string());

        let mut builder = self
            .ctx
            .reporter
            .report(DiagnosticKind::FieldSequenceValue, value.text_range());
        builder = builder.message(field_name);

        if let Expr::Ref(r) = value
            && let Some(name_tok) = r.name()
        {
            let name = name_tok.text();
            if let Some((src, body)) = self.ctx.symbol_table.get_full(name) {
                builder = builder.related_to(src, body.text_range(), "defined here");
            }
        }

        builder.emit();
    }

    /// Check strict dimensionality rule for * and + quantifiers.
    ///
    /// Two checks:
    /// 1. Multi-element patterns (Arity::Many) without captures can't be scalar arrays
    ///    (applies regardless of is_row_capture - row capture doesn't help here)
    /// 2. Internal captures require a row capture on the quantifier
    ///    (skipped when is_row_capture=true)
    fn check_strict_dimensionality(
        &mut self,
        quant: &QuantifiedExpr,
        inner_info: &TermInfo,
        is_row_capture: bool,
    ) {
        let op = quant
            .operator()
            .map(|t| t.text().to_string())
            .unwrap_or_else(|| "*".to_string());

        // Check 1: Multi-element patterns without captures can't be scalar arrays
        // This check applies even with row capture - you can't meaningfully capture
        // multiple nodes per iteration as a scalar
        if inner_info.arity == Arity::Many && inner_info.flow.is_void() {
            self.ctx
                .reporter
                .report(
                    DiagnosticKind::MultiElementScalarCapture,
                    quant.text_range(),
                )
                .message(format!(
                    "sequence with `{}` matches multiple nodes but has no internal captures",
                    op
                ))
                .emit();
            return;
        }

        // Check 2: Internal captures require row capture (skip if already a row capture)
        if is_row_capture {
            return;
        }

        let TypeFlow::Bubble(type_id) = &inner_info.flow else {
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

        self.ctx
            .reporter
            .report(
                DiagnosticKind::StrictDimensionalityViolation,
                quant.text_range(),
            )
            .message(format!(
                "quantifier `{}` contains captures ({}) but has no struct capture",
                op, captures_str
            ))
            .hint(format!("add a struct capture: `{{...}}{} @name`", op))
            .emit();
    }

    fn parse_quantifier(&self, quant: &QuantifiedExpr) -> QuantifierKind {
        // Shared with `capture_mechanism` and `compile`'s implicit-array gate so the
        // three never disagree on a quantifier's arity. A malformed operator-less
        // quantifier can't reach inference, so the fallback is unreachable in practice.
        quantifier_arity(quant).unwrap_or(QuantifierKind::ZeroOrMore)
    }

    fn flow_to_type(&mut self, flow: &TypeFlow) -> TypeId {
        match flow {
            TypeFlow::Void => TYPE_VOID,
            TypeFlow::Scalar(t) | TypeFlow::Bubble(t) => *t,
        }
    }

    /// Check if a type produces meaningful output for propagation.
    ///
    /// Meaningful outputs are structured types (enums, structs, refs) or arrays/optionals
    /// of such types. Simple `Node[]` from quantified named nodes is NOT meaningful.
    fn produces_output(&self, type_id: TypeId) -> bool {
        let Some(shape) = self.ctx.type_ctx.get_type(type_id) else {
            return false;
        };
        match shape {
            TypeShape::Enum(_) | TypeShape::Struct(_) | TypeShape::Ref(_) => true,
            TypeShape::Array { element, .. } => {
                *element != TYPE_NODE && self.produces_output(*element)
            }
            TypeShape::Optional(inner) => *inner != TYPE_NODE && self.produces_output(*inner),
            TypeShape::Node | TypeShape::String | TypeShape::Void | TypeShape::Custom(_) => false,
        }
    }

    /// Compute flow from merged bubble fields and output-producing children.
    ///
    /// Rules:
    /// - No bubbles, 0 outputs → Void
    /// - No bubbles, 1 output → Forward output (propagate)
    /// - No bubbles, 2+ outputs → Error (ambiguous)
    /// - Bubbles, 0 outputs → Bubble(struct)
    /// - Bubbles, 1+ outputs → Error (require capture)
    fn compute_merged_flow(
        &mut self,
        merged_fields: BTreeMap<Symbol, FieldInfo>,
        output_children: Vec<(TextRange, TypeId)>,
        parent_range: TextRange,
    ) -> TypeFlow {
        let has_bubbles = !merged_fields.is_empty();

        match (has_bubbles, output_children.len()) {
            (false, 0) => TypeFlow::Void,
            (false, 1) => TypeFlow::Scalar(output_children[0].1),
            (false, _) => {
                self.report_ambiguous_outputs(parent_range, &output_children);
                TypeFlow::Void
            }
            (true, 0) => TypeFlow::Bubble(self.ctx.type_ctx.intern_struct(merged_fields)),
            (true, _) => {
                self.report_uncaptured_output_with_captures(&output_children);
                TypeFlow::Bubble(self.ctx.type_ctx.intern_struct(merged_fields))
            }
        }
    }

    fn report_ambiguous_outputs(
        &mut self,
        parent_range: TextRange,
        outputs: &[(TextRange, TypeId)],
    ) {
        let source_id = self.ctx.reporter.source();
        let mut builder = self
            .ctx
            .reporter
            .report(DiagnosticKind::AmbiguousUncapturedOutputs, parent_range)
            .message(format!(
                "{} expressions here produce a value but none is captured",
                outputs.len()
            ));
        for (range, _) in outputs {
            builder = builder.related_to(source_id, *range, "produces a value");
        }
        builder.emit();
    }

    fn report_uncaptured_output_with_captures(&mut self, outputs: &[(TextRange, TypeId)]) {
        for (range, _) in outputs {
            self.ctx
                .reporter
                .report(DiagnosticKind::UncapturedOutputWithCaptures, *range)
                .emit();
        }
    }

    fn report_unify_error(&mut self, range: TextRange, err: &UnifyError) {
        let (kind, msg, hint) = match err {
            UnifyError::ScalarInUntagged => (
                DiagnosticKind::IncompatibleTypes,
                "a branch produces a value but the alternation is unlabeled".to_string(),
                Some("give every branch a branch label for a tagged union, e.g. `[A: ... B: ...]`"),
            ),
            UnifyError::IncompatibleTypes { field } => (
                DiagnosticKind::IncompatibleCaptureTypes,
                self.ctx.interner.resolve(*field).to_string(),
                Some(
                    "make every branch produce the same type, or label the branches for a tagged union",
                ),
            ),
            UnifyError::IncompatibleStructs { field } => (
                DiagnosticKind::IncompatibleStructShapes,
                self.ctx.interner.resolve(*field).to_string(),
                Some("use a tagged union if branches need different fields"),
            ),
            UnifyError::IncompatibleArrayElements { field } => (
                DiagnosticKind::IncompatibleCaptureTypes,
                self.ctx.interner.resolve(*field).to_string(),
                Some("array element types must be compatible across branches"),
            ),
        };

        let mut builder = self.ctx.reporter.report(kind, range).message(msg);
        if let Some(h) = hint {
            builder = builder.hint(h);
        }
        builder.emit();
    }
}

impl Visitor for InferenceVisitor<'_, '_> {
    fn visit_def(&mut self, def: &Def) {
        walk_def(self, def);
    }

    fn visit_expr(&mut self, expr: &Expr) {
        self.infer_expr(expr);
    }

    fn visit_named_node(&mut self, node: &NamedNode) {
        // Bottom-up traversal
        walk_named_node(self, node);
    }

    fn visit_seq_expr(&mut self, seq: &SeqExpr) {
        walk_seq_expr(self, seq);
    }

    fn visit_alt_expr(&mut self, alt: &AltExpr) {
        walk_alt_expr(self, alt);
    }
}

/// Run inference on all definitions in a root.
fn infer_root(ctx: InferenceContext, root: &Root) {
    let mut visitor = InferenceVisitor::new(ctx);
    visitor.visit(root);
}

/// Orchestrates type inference across all definitions in dependency order.
pub(super) struct InferencePass<'a> {
    ctx: TypeContext,
    interner: &'a mut Interner,
    ast_map: &'a IndexMap<SourceId, Root>,
    symbol_table: &'a SymbolTable,
    dependency_analysis: &'a DependencyAnalysis,
    diag: &'a mut Diagnostics,
}

impl<'a> InferencePass<'a> {
    pub fn new(
        interner: &'a mut Interner,
        ast_map: &'a IndexMap<SourceId, Root>,
        symbol_table: &'a SymbolTable,
        dependency_analysis: &'a DependencyAnalysis,
        diag: &'a mut Diagnostics,
    ) -> Self {
        Self {
            ctx: TypeContext::new(),
            interner,
            ast_map,
            symbol_table,
            dependency_analysis,
            diag,
        }
    }

    pub fn run(mut self) -> TypeContext {
        // Avoid re-registration of definitions
        self.ctx.seed_defs(
            self.dependency_analysis.def_names(),
            self.dependency_analysis.name_to_def(),
        );

        self.mark_recursion();
        self.process_sccs();
        self.process_orphans();

        self.ctx
    }

    /// Identify and mark recursive definitions.
    fn mark_recursion(&mut self) {
        for scc in self.dependency_analysis.sccs() {
            for def_name in scc {
                if !self.dependency_analysis.is_recursive(def_name) {
                    continue;
                }
                let sym = self.interner.intern(def_name);
                let Some(def_id) = self.ctx.get_def_id_sym(sym) else {
                    continue;
                };
                self.ctx.mark_recursive(def_id);
            }
        }
    }

    /// Process definitions in SCC order (leaves first).
    fn process_sccs(&mut self) {
        for scc in self.dependency_analysis.sccs() {
            for def_name in scc {
                if let Some(source_id) = self.symbol_table.source_id(def_name) {
                    self.infer_and_register(def_name, source_id);
                }
            }
        }
    }

    /// Handle any definitions not in an SCC (safety net).
    fn process_orphans(&mut self) {
        for (name, source_id, _body) in self.symbol_table.iter_full() {
            // Skip if already processed
            if self.ctx.get_def_type_by_name(self.interner, name).is_some() {
                continue;
            }
            self.infer_and_register(name, source_id);
        }
    }

    fn infer_and_register(&mut self, def_name: &str, source_id: SourceId) {
        let Some(root) = self.ast_map.get(&source_id) else {
            return;
        };

        infer_root(
            InferenceContext {
                type_ctx: &mut self.ctx,
                interner: self.interner,
                symbol_table: self.symbol_table,
                reporter: Reporter::new(source_id, self.diag),
            },
            root,
        );

        if let Some(body) = self.symbol_table.get(def_name)
            && let Some(info) = self.ctx.get_term_info(body).cloned()
        {
            let type_id = self.flow_to_type_id(&info.flow);
            self.ctx
                .set_def_type_by_name(self.interner, def_name, type_id);
        }
    }

    fn flow_to_type_id(&mut self, flow: &TypeFlow) -> TypeId {
        match flow {
            TypeFlow::Void => TYPE_VOID,
            TypeFlow::Scalar(id) | TypeFlow::Bubble(id) => *id,
        }
    }
}
