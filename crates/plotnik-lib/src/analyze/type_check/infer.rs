//! Bottom-up type inference visitor.
//!
//! Traverses the AST and computes TermInfo (Arity + TypeFlow) for each expression.
//! Reports diagnostics for type errors like strict dimensionality violations.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use plotnik_core::Interner;
use rowan::TextRange;

use super::context::TypeContext;
use super::symbol::Symbol;
use super::types::{
    Arity, FieldInfo, QuantifierKind, TYPE_NODE, TYPE_STRING, TYPE_VOID, TermInfo, TypeFlow,
    TypeId, TypeShape,
};
use super::unify::{UnifyError, unify_flows};

use crate::analyze::symbol_table::SymbolTable;
use crate::analyze::visitor::{Visitor, walk_alt_expr, walk_def, walk_named_node, walk_seq_expr};
use crate::diagnostics::{DiagnosticKind, Diagnostics};
use crate::parser::ast::{
    AltExpr, AltKind, AnonymousNode, CapturedExpr, Def, Expr, FieldExpr, NamedNode, QuantifiedExpr,
    Ref, Root, SeqExpr, is_truly_empty_scope,
};
use crate::parser::cst::SyntaxKind;
use crate::query::source_map::SourceId;

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

/// Inference context for a single pass over the AST.
pub struct InferenceVisitor<'a, 'd> {
    pub ctx: &'a mut TypeContext,
    pub interner: &'a mut Interner,
    pub symbol_table: &'a SymbolTable,
    pub source_id: SourceId,
    pub diag: &'d mut Diagnostics,
}

impl<'a, 'd> InferenceVisitor<'a, 'd> {
    pub fn new(
        ctx: &'a mut TypeContext,
        interner: &'a mut Interner,
        symbol_table: &'a SymbolTable,
        source_id: SourceId,
        diag: &'d mut Diagnostics,
    ) -> Self {
        Self {
            ctx,
            interner,
            symbol_table,
            source_id,
            diag,
        }
    }

    /// Infer the TermInfo for an expression, caching the result.
    pub fn infer_expr(&mut self, expr: &Expr) -> TermInfo {
        if let Some(info) = self.ctx.get_term_info(expr) {
            return info.clone();
        }

        // Sentinel to break recursion cycles
        self.ctx.set_term_info(expr.clone(), TermInfo::void());

        let info = self.compute_expr(expr);
        self.ctx.set_term_info(expr.clone(), info.clone());
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
                    if let Some(fields) = self.ctx.get_struct_fields(*type_id) {
                        for (name, info) in fields {
                            merged_fields.entry(*name).or_insert(*info);
                        }
                    }
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
        let name_sym = self.interner.intern(name);

        // Recursive refs are opaque boundaries - they match but don't bubble captures.
        // The Ref type is created when a recursive ref is captured (in infer_captured_expr).
        if let Some(def_id) = self.ctx.get_def_id_sym(name_sym)
            && self.ctx.is_recursive(def_id)
        {
            return TermInfo::new(Arity::One, TypeFlow::Void);
        }

        let Some(body) = self.symbol_table.get(name) else {
            return TermInfo::void();
        };

        // Non-recursive refs are transparent
        self.infer_expr(body)
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
                    if let Some(fields) = self.ctx.get_struct_fields(*type_id).cloned() {
                        self.merge_seq_fields(&mut merged_fields, &fields, child.text_range());
                    }
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

    fn merge_seq_fields(
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
                    self.diag
                        .report(
                            self.source_id,
                            DiagnosticKind::DuplicateCaptureInScope,
                            range,
                        )
                        .message(self.interner.resolve(name))
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
            let label_sym = self.interner.intern(label.text());

            let Some(body) = branch.body() else {
                // Empty variant -> Void (no payload)
                variants.insert(label_sym, TYPE_VOID);
                continue;
            };

            let body_info = self.infer_expr(&body);
            combined_arity = combined_arity.combine(body_info.arity);
            variants.insert(label_sym, self.flow_to_type(&body_info.flow));
        }

        let enum_type = self.ctx.intern_type(TypeShape::Enum(variants));
        TermInfo::new(combined_arity, TypeFlow::Scalar(enum_type))
    }

    fn infer_untagged_alt(&mut self, alt: &AltExpr) -> TermInfo {
        let mut flows: Vec<TypeFlow> = Vec::new();
        let mut combined_arity = Arity::One;

        // Collect from branches
        for branch in alt.branches() {
            if let Some(body) = branch.body() {
                let info = self.infer_expr(&body);
                combined_arity = combined_arity.combine(info.arity);
                flows.push(info.flow);
            }
        }

        // Collect from direct expressions
        for expr in alt.exprs() {
            let info = self.infer_expr(&expr);
            combined_arity = combined_arity.combine(info.arity);
            flows.push(info.flow);
        }

        let unified_flow = match unify_flows(self.ctx, flows) {
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
        let capture_name = self.interner.intern(&name_tok.text()[1..]); // Strip @ prefix

        let annotation = self.resolve_annotation(cap);
        let Some(inner) = cap.inner() else {
            // Capture without inner -> creates a Node field with optional annotation
            let type_id = self.annotation_to_alias(annotation, TYPE_NODE);
            let field = FieldInfo::required(type_id);
            return TermInfo::new(
                Arity::One,
                TypeFlow::Bubble(self.ctx.intern_single_field(capture_name, field)),
            );
        };

        // Determine how inner flow relates to capture (e.g., ? makes field optional)
        let (inner_info, is_optional) = self.resolve_capture_inner(&inner);

        // Determine if we need to merge bubbling fields with the capture.
        // Only applies when inner has Bubble flow AND doesn't create a scope boundary.
        // Sequences and alternations create scopes; named nodes/refs don't.
        let should_merge_fields =
            matches!(&inner_info.flow, TypeFlow::Bubble(_)) && !Self::inner_creates_scope(&inner);

        if should_merge_fields {
            // Named node/ref/etc with bubbling fields: capture adds a field,
            // inner fields bubble up alongside.
            let captured_type = self.determine_non_scope_captured_type(&inner, annotation);
            let field_info = if is_optional {
                FieldInfo::optional(captured_type)
            } else {
                FieldInfo::required(captured_type)
            };

            // Merge capture field with inner's bubbling fields
            let TypeFlow::Bubble(type_id) = &inner_info.flow else {
                unreachable!()
            };
            let mut fields = self
                .ctx
                .get_struct_fields(*type_id)
                .cloned()
                .unwrap_or_default();
            fields.insert(capture_name, field_info);

            TermInfo::new(
                inner_info.arity,
                TypeFlow::Bubble(self.ctx.intern_struct(fields)),
            )
        } else {
            // All other cases: scope-creating captures, scalar flows, void flows.
            // Inner becomes the captured type (if applicable).
            let captured_type = self.determine_captured_type(&inner, &inner_info, annotation);
            let field_info = if is_optional {
                FieldInfo::optional(captured_type)
            } else {
                FieldInfo::required(captured_type)
            };
            TermInfo::new(
                inner_info.arity,
                TypeFlow::Bubble(self.ctx.intern_single_field(capture_name, field_info)),
            )
        }
    }

    /// Determines if an expression creates a scope boundary when captured.
    ///
    /// When captured, these expressions produce structured values (not nodes):
    /// - Sequences/alternations: produce structs/enums from their internal captures
    /// - Refs: produce whatever the called definition returns (struct if it has captures)
    ///
    /// This only affects captured expressions. Uncaptured refs remain transparent
    /// (their captures bubble up) because this check only runs in `infer_captured_expr`.
    fn inner_creates_scope(inner: &Expr) -> bool {
        match inner {
            Expr::SeqExpr(_) | Expr::AltExpr(_) | Expr::Ref(_) => true,
            Expr::QuantifiedExpr(q) => {
                // Look through quantifier to the actual expression
                q.inner()
                    .map(|i| Self::inner_creates_scope(&i))
                    .unwrap_or(false)
            }
            _ => false,
        }
    }

    /// Determines captured type for non-scope-creating expressions.
    ///
    /// For non-scope captures, fields bubble up alongside the capture field.
    /// The annotation applies to the capture's type (usually Node or a recursive ref).
    fn determine_non_scope_captured_type(
        &mut self,
        inner: &Expr,
        annotation: Option<AnnotationKind>,
    ) -> TypeId {
        let base_type = self.get_recursive_ref_type(inner).unwrap_or(TYPE_NODE);
        self.annotation_to_alias(annotation, base_type)
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
                    AnnotationKind::TypeName(self.interner.intern(text))
                }
            })
        })
    }

    /// Converts annotation to a type, creating a Node alias for custom type names.
    ///
    /// Used for non-struct contexts where TypeName should create an alias to Node.
    fn annotation_to_alias(&mut self, annotation: Option<AnnotationKind>, base: TypeId) -> TypeId {
        match annotation {
            Some(AnnotationKind::String) => TYPE_STRING,
            Some(AnnotationKind::TypeName(name)) => self.ctx.intern_type(TypeShape::Custom(name)),
            None => base,
        }
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

    /// Transforms the inner flow into a specific TypeId for the field.
    ///
    /// Handles type annotation semantics based on the flow:
    /// - Void/Scalar + TypeName: creates a Node alias (current Custom behavior)
    /// - Bubble + TypeName: names the struct type instead of replacing it
    fn determine_captured_type(
        &mut self,
        inner: &Expr,
        inner_info: &TermInfo,
        annotation: Option<AnnotationKind>,
    ) -> TypeId {
        match &inner_info.flow {
            TypeFlow::Void => {
                // Truly empty sequences/alternations produce empty struct.
                // E.g., `{ } @x` has type `{ x: {} }`.
                // Non-empty sequences with void flow (e.g., suppressed captures)
                // still produce Node for the capture.
                if is_truly_empty_scope(inner) {
                    let empty_struct = self.ctx.intern_struct(BTreeMap::new());
                    match annotation {
                        Some(AnnotationKind::String) => TYPE_STRING,
                        Some(AnnotationKind::TypeName(name)) => {
                            self.ctx.set_type_name(empty_struct, name);
                            empty_struct
                        }
                        None => empty_struct,
                    }
                } else {
                    let base_type = self.get_recursive_ref_type(inner).unwrap_or(TYPE_NODE);
                    self.annotation_to_alias(annotation, base_type)
                }
            }
            TypeFlow::Scalar(type_id) => {
                // For array types with annotation, replace the element type
                // e.g., `(identifier)* @names :: string` → string[] not string
                if let Some(AnnotationKind::String) = annotation
                    && let Some(TypeShape::Array { non_empty, .. }) = self.ctx.get_type(*type_id)
                {
                    return self.ctx.intern_type(TypeShape::Array {
                        element: TYPE_STRING,
                        non_empty: *non_empty,
                    });
                }
                match annotation {
                    Some(AnnotationKind::String) => TYPE_STRING,
                    Some(AnnotationKind::TypeName(name)) => {
                        // For enum types, name the enum instead of creating an alias
                        if matches!(self.ctx.get_type(*type_id), Some(TypeShape::Enum(_))) {
                            self.ctx.set_type_name(*type_id, name);
                            *type_id
                        } else {
                            self.ctx.intern_type(TypeShape::Custom(name))
                        }
                    }
                    None => *type_id,
                }
            }
            TypeFlow::Bubble(type_id) => {
                // Bubble flow means inner has struct fields (scope-creating capture).
                // TypeName annotation should NAME the struct, not replace it with an alias.
                match annotation {
                    Some(AnnotationKind::String) => TYPE_STRING,
                    Some(AnnotationKind::TypeName(name)) => {
                        // Register the name for this struct type
                        self.ctx.set_type_name(*type_id, name);
                        *type_id
                    }
                    None => *type_id,
                }
            }
        }
    }

    /// If expr is (or contains) a recursive Ref, return its Ref type.
    fn get_recursive_ref_type(&mut self, expr: &Expr) -> Option<TypeId> {
        match expr {
            Expr::Ref(r) => {
                let name_tok = r.name()?;
                let name = name_tok.text();
                let sym = self.interner.intern(name);
                let def_id = self.ctx.get_def_id_sym(sym)?;
                if self.ctx.is_recursive(def_id) {
                    Some(self.ctx.intern_type(TypeShape::Ref(def_id)))
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
                if !is_row_capture {
                    self.check_strict_dimensionality(quant, &inner_info);
                }
                self.make_flow_array(inner_info.flow, &inner, quantifier.is_non_empty())
            }
        };

        TermInfo::new(inner_info.arity, flow)
    }

    fn make_flow_optional(&mut self, flow: TypeFlow) -> TypeFlow {
        match flow {
            TypeFlow::Void => TypeFlow::Void,
            TypeFlow::Scalar(t) => TypeFlow::Scalar(self.ctx.intern_type(TypeShape::Optional(t))),
            TypeFlow::Bubble(type_id) => {
                let fields = self
                    .ctx
                    .get_struct_fields(type_id)
                    .cloned()
                    .unwrap_or_default();
                let optional_fields = fields
                    .into_iter()
                    .map(|(k, v)| (k, v.make_optional()))
                    .collect();
                TypeFlow::Bubble(self.ctx.intern_struct(optional_fields))
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
                    .intern_type(TypeShape::Array { element, non_empty });
                TypeFlow::Scalar(array_type)
            }
            TypeFlow::Scalar(t) => {
                let array_type = self.ctx.intern_type(TypeShape::Array {
                    element: t,
                    non_empty,
                });
                TypeFlow::Scalar(array_type)
            }
            TypeFlow::Bubble(struct_type) => {
                // Note: Bubble with * or + is strictly invalid unless it's a row capture,
                // but we construct a valid type as fallback.
                let array_type = self.ctx.intern_type(TypeShape::Array {
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

        let mut builder = self.diag.report(
            self.source_id,
            DiagnosticKind::FieldSequenceValue,
            value.text_range(),
        );
        builder = builder.message(field_name);

        if let Expr::Ref(r) = value
            && let Some(name_tok) = r.name()
        {
            let name = name_tok.text();
            if let Some((src, body)) = self.symbol_table.get_full(name) {
                builder = builder.related_to(src, body.text_range(), "defined here");
            }
        }

        builder.emit();
    }

    /// Check strict dimensionality rule for * and + quantifiers.
    /// Captures inside a quantifier are forbidden unless marked as a row capture.
    fn check_strict_dimensionality(&mut self, quant: &QuantifiedExpr, inner_info: &TermInfo) {
        let TypeFlow::Bubble(type_id) = &inner_info.flow else {
            return;
        };

        let fields = self
            .ctx
            .get_struct_fields(*type_id)
            .expect("Bubble flow must point to a Struct type");
        if fields.is_empty() {
            return;
        }

        let op = quant
            .operator()
            .map(|t| t.text().to_string())
            .unwrap_or_else(|| "*".to_string());

        let capture_names: Vec<_> = fields
            .keys()
            .map(|s| format!("`@{}`", self.interner.resolve(*s)))
            .collect();
        let captures_str = capture_names.join(", ");

        self.diag
            .report(
                self.source_id,
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
        let Some(op) = quant.operator() else {
            return QuantifierKind::ZeroOrMore;
        };

        match op.kind() {
            SyntaxKind::Question | SyntaxKind::QuestionQuestion => QuantifierKind::Optional,
            SyntaxKind::Star | SyntaxKind::StarQuestion => QuantifierKind::ZeroOrMore,
            SyntaxKind::Plus | SyntaxKind::PlusQuestion => QuantifierKind::OneOrMore,
            _ => QuantifierKind::ZeroOrMore,
        }
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
        let Some(shape) = self.ctx.get_type(type_id) else {
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
            (true, 0) => TypeFlow::Bubble(self.ctx.intern_struct(merged_fields)),
            (true, _) => {
                self.report_uncaptured_output_with_captures(&output_children);
                TypeFlow::Bubble(self.ctx.intern_struct(merged_fields))
            }
        }
    }

    fn report_ambiguous_outputs(
        &mut self,
        parent_range: TextRange,
        outputs: &[(TextRange, TypeId)],
    ) {
        self.diag
            .report(
                self.source_id,
                DiagnosticKind::AmbiguousUncapturedOutputs,
                parent_range,
            )
            .message(format!(
                "{} expressions produce output without capture",
                outputs.len()
            ))
            .emit();
    }

    fn report_uncaptured_output_with_captures(&mut self, outputs: &[(TextRange, TypeId)]) {
        for (range, _) in outputs {
            self.diag
                .report(
                    self.source_id,
                    DiagnosticKind::UncapturedOutputWithCaptures,
                    *range,
                )
                .emit();
        }
    }

    fn report_unify_error(&mut self, range: TextRange, err: &UnifyError) {
        let (kind, msg, hint) = match err {
            UnifyError::ScalarInUntagged => (
                DiagnosticKind::IncompatibleTypes,
                "scalar type in untagged alternation".to_string(),
                Some("use tagged alternation if branches need different types"),
            ),
            UnifyError::IncompatibleTypes { field } => (
                DiagnosticKind::IncompatibleCaptureTypes,
                self.interner.resolve(*field).to_string(),
                Some("all branches must produce the same type for merged captures"),
            ),
            UnifyError::IncompatibleStructs { field } => (
                DiagnosticKind::IncompatibleStructShapes,
                self.interner.resolve(*field).to_string(),
                Some("use tagged alternation if branches need different fields"),
            ),
            UnifyError::IncompatibleArrayElements { field } => (
                DiagnosticKind::IncompatibleCaptureTypes,
                self.interner.resolve(*field).to_string(),
                Some("array element types must be compatible across branches"),
            ),
        };

        let mut builder = self.diag.report(self.source_id, kind, range).message(msg);
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
pub fn infer_root(
    ctx: &mut TypeContext,
    interner: &mut Interner,
    symbol_table: &SymbolTable,
    source_id: SourceId,
    root: &Root,
    diag: &mut Diagnostics,
) {
    let mut visitor = InferenceVisitor::new(ctx, interner, symbol_table, source_id, diag);
    visitor.visit(root);
}
