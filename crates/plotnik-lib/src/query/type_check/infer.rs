//! Bottom-up type inference visitor.
//!
//! Traverses the AST and computes TermInfo (Arity + TypeFlow) for each expression.
//! Reports diagnostics for type errors like strict dimensionality violations.

use std::collections::BTreeMap;

use rowan::TextRange;

use crate::diagnostics::{DiagnosticKind, Diagnostics};
use crate::parser::ast::{
    AltExpr, AltKind, AnonymousNode, CapturedExpr, Def, Expr, FieldExpr, NamedNode, QuantifiedExpr,
    Ref, Root, SeqExpr,
};
use crate::parser::cst::SyntaxKind;
use crate::query::source_map::SourceId;
use crate::query::symbol_table::SymbolTable;
use crate::query::visitor::{Visitor, walk_alt_expr, walk_def, walk_named_node, walk_seq_expr};

use super::context::TypeContext;
use super::types::{
    Arity, FieldInfo, QuantifierKind, TYPE_NODE, TYPE_STRING, TermInfo, TypeFlow, TypeId, TypeKind,
};
use super::unify::{UnifyError, unify_flows};

/// Inference context for a single pass over the AST.
pub struct InferenceVisitor<'a, 'd> {
    pub ctx: &'a mut TypeContext,
    pub symbol_table: &'a SymbolTable,
    pub diag: &'d mut Diagnostics,
    pub source_id: SourceId,
}

impl<'a, 'd> InferenceVisitor<'a, 'd> {
    pub fn new(
        ctx: &'a mut TypeContext,
        symbol_table: &'a SymbolTable,
        diag: &'d mut Diagnostics,
        source_id: SourceId,
    ) -> Self {
        Self {
            ctx,
            symbol_table,
            diag,
            source_id,
        }
    }

    /// Infer the TermInfo for an expression, caching the result.
    pub fn infer_expr(&mut self, expr: &Expr) -> TermInfo {
        // Check cache first
        if let Some(info) = self.ctx.get_term_info(expr) {
            return info.clone();
        }

        // Insert sentinel to break cycles
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

    /// Named node: matches one position, produces nothing
    fn infer_named_node(&mut self, node: &NamedNode) -> TermInfo {
        // Recursively infer children first
        for child in node.children() {
            self.infer_expr(&child);
        }
        TermInfo::new(Arity::One, TypeFlow::Void)
    }

    /// Anonymous node (literal or wildcard): matches one position, produces nothing
    fn infer_anonymous_node(&mut self, _node: &AnonymousNode) -> TermInfo {
        TermInfo::new(Arity::One, TypeFlow::Void)
    }

    /// Reference: delegate arity, but refs are scope boundaries so produce Scalar(Ref)
    fn infer_ref(&mut self, r: &Ref) -> TermInfo {
        let Some(name_tok) = r.name() else {
            return TermInfo::void();
        };
        let name = name_tok.text();

        // Get the body expression for this definition
        let Some(body) = self.symbol_table.get(name) else {
            // Undefined ref - already reported by symbol_table pass
            return TermInfo::void();
        };

        // Infer the body to get its arity
        let body_info = self.infer_expr(body);

        // Refs are scope boundaries - they produce a Scalar(Ref) regardless of what's inside
        let ref_type = self.ctx.intern_type(TypeKind::Ref(name.to_string()));
        TermInfo::new(body_info.arity, TypeFlow::Scalar(ref_type))
    }

    /// Sequence: One if 0-1 children, else Many; merge children's fields
    fn infer_seq_expr(&mut self, seq: &SeqExpr) -> TermInfo {
        let children: Vec<_> = seq.children().collect();

        // Compute arity based on child count
        let arity = match children.len() {
            0 | 1 => children
                .first()
                .map(|c| self.infer_expr(c).arity)
                .unwrap_or(Arity::One),
            _ => Arity::Many,
        };

        // Merge fields from all children
        let mut merged_fields: BTreeMap<String, FieldInfo> = BTreeMap::new();

        for child in &children {
            let child_info = self.infer_expr(child);

            if let TypeFlow::Fields(fields) = child_info.flow {
                for (name, info) in fields {
                    if merged_fields.contains_key(&name) {
                        // Duplicate capture in same scope - error
                        self.diag
                            .report(
                                self.source_id,
                                DiagnosticKind::DuplicateCaptureInScope,
                                child.text_range(),
                            )
                            .message(&name)
                            .emit();
                    } else {
                        merged_fields.insert(name, info);
                    }
                }
            }
            // Void and Scalar children don't contribute fields
            // (Scalar would be from refs, which are scope boundaries)
        }

        let flow = if merged_fields.is_empty() {
            TypeFlow::Void
        } else {
            TypeFlow::Fields(merged_fields)
        };

        TermInfo::new(arity, flow)
    }

    /// Alternation: arity is Many if ANY branch is Many; type depends on tagged vs untagged
    fn infer_alt_expr(&mut self, alt: &AltExpr) -> TermInfo {
        let kind = alt.kind();

        match kind {
            AltKind::Tagged => self.infer_tagged_alt(alt),
            AltKind::Untagged | AltKind::Mixed => self.infer_untagged_alt(alt),
        }
    }

    fn infer_tagged_alt(&mut self, alt: &AltExpr) -> TermInfo {
        let mut variants: BTreeMap<String, TypeId> = BTreeMap::new();
        let mut combined_arity = Arity::One;

        for branch in alt.branches() {
            let Some(label) = branch.label() else {
                continue;
            };
            let label_text = label.text().to_string();

            let Some(body) = branch.body() else {
                // Empty variant gets void/empty struct type
                variants.insert(
                    label_text,
                    self.ctx.intern_type(TypeKind::Struct(BTreeMap::new())),
                );
                continue;
            };

            let body_info = self.infer_expr(&body);
            combined_arity = combined_arity.combine(body_info.arity);

            // Convert flow to a type for this variant
            let variant_type = self.flow_to_type(&body_info.flow);
            variants.insert(label_text, variant_type);
        }

        // Tagged alternation produces an Enum type
        let enum_type = self.ctx.intern_type(TypeKind::Enum(variants));
        TermInfo::new(combined_arity, TypeFlow::Scalar(enum_type))
    }

    fn infer_untagged_alt(&mut self, alt: &AltExpr) -> TermInfo {
        let mut flows: Vec<TypeFlow> = Vec::new();
        let mut combined_arity = Arity::One;

        // Handle both direct exprs and branches without labels
        for branch in alt.branches() {
            if let Some(body) = branch.body() {
                let body_info = self.infer_expr(&body);
                combined_arity = combined_arity.combine(body_info.arity);
                flows.push(body_info.flow);
            }
        }

        for expr in alt.exprs() {
            let expr_info = self.infer_expr(&expr);
            combined_arity = combined_arity.combine(expr_info.arity);
            flows.push(expr_info.flow);
        }

        // Unify all flows
        let unified_flow = match unify_flows(flows) {
            Ok(flow) => flow,
            Err(err) => {
                self.report_unify_error(alt.text_range(), &err);
                TypeFlow::Void
            }
        };

        TermInfo::new(combined_arity, unified_flow)
    }

    /// Captured expression: wraps inner's flow into a field
    fn infer_captured_expr(&mut self, cap: &CapturedExpr) -> TermInfo {
        let Some(name_tok) = cap.name() else {
            // Missing name - recover gracefully
            return cap
                .inner()
                .map(|inner| self.infer_expr(&inner))
                .unwrap_or_else(TermInfo::void);
        };
        let capture_name = name_tok.text().to_string();

        // Check for type annotation
        let annotation_type = cap.type_annotation().and_then(|t| {
            t.name().map(|n| {
                let type_name = n.text();
                if type_name == "string" {
                    TYPE_STRING
                } else {
                    self.ctx
                        .intern_type(TypeKind::Custom(type_name.to_string()))
                }
            })
        });

        let Some(inner) = cap.inner() else {
            // Capture without inner - still produces a field
            let type_id = annotation_type.unwrap_or(TYPE_NODE);
            return TermInfo::new(
                Arity::One,
                TypeFlow::single_field(capture_name, FieldInfo::required(type_id)),
            );
        };

        let inner_info = self.infer_expr(&inner);

        // Transform based on inner's flow
        let captured_type = match &inner_info.flow {
            TypeFlow::Void => {
                // @name on Void → capture produces Node (or annotated type)
                annotation_type.unwrap_or(TYPE_NODE)
            }
            TypeFlow::Scalar(type_id) => {
                // @name on Scalar → capture that scalar type
                annotation_type.unwrap_or(*type_id)
            }
            TypeFlow::Fields(fields) => {
                // @name on Fields → create Struct from fields, capture that
                if let Some(annotated) = annotation_type {
                    annotated
                } else {
                    self.ctx.intern_type(TypeKind::Struct(fields.clone()))
                }
            }
        };

        TermInfo::new(
            inner_info.arity,
            TypeFlow::single_field(capture_name, FieldInfo::required(captured_type)),
        )
    }

    /// Quantified expression: applies quantifier to inner's flow
    fn infer_quantified_expr(&mut self, quant: &QuantifiedExpr) -> TermInfo {
        let Some(inner) = quant.inner() else {
            return TermInfo::void();
        };

        let inner_info = self.infer_expr(&inner);
        let quantifier = self.parse_quantifier(quant);

        match quantifier {
            QuantifierKind::Optional => {
                // `?` makes fields optional, doesn't add dimensionality
                let flow = match inner_info.flow {
                    TypeFlow::Void => TypeFlow::Void,
                    TypeFlow::Scalar(t) => {
                        TypeFlow::Scalar(self.ctx.intern_type(TypeKind::Optional(t)))
                    }
                    TypeFlow::Fields(fields) => {
                        // Make all fields optional
                        let optional_fields = fields
                            .into_iter()
                            .map(|(k, v)| (k, v.make_optional()))
                            .collect();
                        TypeFlow::Fields(optional_fields)
                    }
                };
                TermInfo::new(inner_info.arity, flow)
            }

            QuantifierKind::ZeroOrMore | QuantifierKind::OneOrMore => {
                // * and + require strict dimensionality
                self.check_strict_dimensionality(quant, &inner_info);

                let flow = match inner_info.flow {
                    TypeFlow::Void => TypeFlow::Void,
                    TypeFlow::Scalar(t) => {
                        // Scalar becomes array
                        let array_type = self.ctx.intern_type(TypeKind::Array {
                            element: t,
                            non_empty: quantifier.is_non_empty(),
                        });
                        TypeFlow::Scalar(array_type)
                    }
                    TypeFlow::Fields(fields) => {
                        // Fields with * or + and no row capture is an error
                        // (already reported by check_strict_dimensionality)
                        // Return array of struct as best-effort
                        let struct_type = self.ctx.intern_type(TypeKind::Struct(fields));
                        let array_type = self.ctx.intern_type(TypeKind::Array {
                            element: struct_type,
                            non_empty: quantifier.is_non_empty(),
                        });
                        TypeFlow::Scalar(array_type)
                    }
                };
                TermInfo::new(inner_info.arity, flow)
            }
        }
    }

    /// Field expression: arity One, delegates type to value
    fn infer_field_expr(&mut self, field: &FieldExpr) -> TermInfo {
        let Some(value) = field.value() else {
            return TermInfo::void();
        };

        let value_info = self.infer_expr(&value);

        // Field validation: value must have arity One
        if value_info.arity == Arity::Many {
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

            // If value is a reference, add related info
            if let Expr::Ref(r) = &value
                && let Some(name_tok) = r.name()
                && let Some((def_source, def_body)) = self.symbol_table.get_full(name_tok.text())
            {
                builder = builder.related_to(def_source, def_body.text_range(), "defined here");
            }

            builder.emit();
        }

        // Field itself has arity One; flow passes through
        TermInfo::new(Arity::One, value_info.flow)
    }

    /// Check strict dimensionality rule for * and + quantifiers.
    fn check_strict_dimensionality(&mut self, quant: &QuantifiedExpr, inner_info: &TermInfo) {
        // If inner has fields (captures), that's a violation
        if let TypeFlow::Fields(fields) = &inner_info.flow
            && !fields.is_empty()
        {
            let op = quant
                .operator()
                .map(|t| t.text().to_string())
                .unwrap_or_else(|| "*".to_string());

            let capture_names: Vec<_> = fields.keys().map(|s| format!("`@{}`", s)).collect();
            let captures_str = capture_names.join(", ");

            self.diag
                .report(
                    self.source_id,
                    DiagnosticKind::StrictDimensionalityViolation,
                    quant.text_range(),
                )
                .message(format!(
                    "quantifier `{}` contains captures ({}) but no row capture",
                    op, captures_str
                ))
                .hint("wrap as `{...}* @rows`")
                .emit();
        }
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

    /// Convert a TypeFlow to a TypeId for storage in enum variants, etc.
    fn flow_to_type(&mut self, flow: &TypeFlow) -> TypeId {
        match flow {
            TypeFlow::Void => self.ctx.intern_type(TypeKind::Struct(BTreeMap::new())),
            TypeFlow::Scalar(t) => *t,
            TypeFlow::Fields(fields) => self.ctx.intern_type(TypeKind::Struct(fields.clone())),
        }
    }

    fn report_unify_error(&mut self, range: TextRange, err: &UnifyError) {
        let (kind, msg) = match err {
            UnifyError::ScalarInUntagged => (
                DiagnosticKind::IncompatibleTypes,
                "scalar type in untagged alternation; use tagged alternation instead".to_string(),
            ),
            UnifyError::IncompatibleTypes { field } => {
                (DiagnosticKind::IncompatibleCaptureTypes, field.clone())
            }
            UnifyError::IncompatibleStructs { field } => {
                (DiagnosticKind::IncompatibleStructShapes, field.clone())
            }
            UnifyError::IncompatibleArrayElements { field } => {
                (DiagnosticKind::IncompatibleCaptureTypes, field.clone())
            }
        };

        self.diag
            .report(self.source_id, kind, range)
            .message(msg)
            .emit();
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
        // Visit children first (bottom-up)
        walk_named_node(self, node);
    }

    fn visit_seq_expr(&mut self, seq: &SeqExpr) {
        walk_seq_expr(self, seq);
    }

    fn visit_alt_expr(&mut self, alt: &AltExpr) {
        walk_alt_expr(self, alt);
    }
}

/// Run inference on a single definition.
pub fn infer_definition(
    ctx: &mut TypeContext,
    symbol_table: &SymbolTable,
    diag: &mut Diagnostics,
    source_id: SourceId,
    def_name: &str,
) -> Option<TermInfo> {
    let body = symbol_table.get(def_name)?;
    let mut visitor = InferenceVisitor::new(ctx, symbol_table, diag, source_id);
    Some(visitor.infer_expr(body))
}

/// Run inference on all definitions in a root.
pub fn infer_root(
    ctx: &mut TypeContext,
    symbol_table: &SymbolTable,
    diag: &mut Diagnostics,
    source_id: SourceId,
    root: &Root,
) {
    let mut visitor = InferenceVisitor::new(ctx, symbol_table, diag, source_id);
    visitor.visit(root);
}
