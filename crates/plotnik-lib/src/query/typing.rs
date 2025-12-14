//! AST-based type inference for Plotnik queries.
//!
//! Analyzes query AST to determine output types.
//! Rules follow ADR-0009 (Type System).
//!
//! # Design
//!
//! Unlike graph-based inference which must reconstruct structure from CFG traversal,
//! AST-based inference directly walks the tree structure:
//! - Sequences → `SeqExpr`
//! - Alternations → `AltExpr` with `.kind()` for tagged/untagged
//! - Quantifiers → `QuantifiedExpr`
//! - Captures → `CapturedExpr`
//!
//! This eliminates dry-run traversal, reconvergence detection, and scope stack management.

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;
use rowan::TextRange;

use crate::diagnostics::{DiagnosticKind, Diagnostics};
use crate::ir::{TYPE_NODE, TYPE_STR, TYPE_VOID, TypeId, TypeKind};
use crate::parser::ast::{self, AltKind, Expr};
use crate::parser::token_src;

use super::Query;

/// Result of type inference.
#[derive(Debug, Default)]
pub struct TypeInferenceResult<'src> {
    pub type_defs: Vec<InferredTypeDef<'src>>,
    pub entrypoint_types: IndexMap<&'src str, TypeId>,
    pub diagnostics: Diagnostics,
    pub errors: Vec<UnificationError<'src>>,
}

/// Error when types cannot be unified in alternation branches.
#[derive(Debug, Clone)]
pub struct UnificationError<'src> {
    pub field: &'src str,
    pub definition: &'src str,
    pub types_found: Vec<TypeDescription>,
    pub spans: Vec<TextRange>,
}

/// Human-readable type description for error messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeDescription {
    Node,
    String,
    Struct(Vec<String>),
}

impl std::fmt::Display for TypeDescription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TypeDescription::Node => write!(f, "Node"),
            TypeDescription::String => write!(f, "String"),
            TypeDescription::Struct(fields) => {
                write!(f, "Struct {{ {} }}", fields.join(", "))
            }
        }
    }
}

/// An inferred type definition.
#[derive(Debug, Clone)]
pub struct InferredTypeDef<'src> {
    pub kind: TypeKind,
    pub name: Option<&'src str>,
    pub members: Vec<InferredMember<'src>>,
    pub inner_type: Option<TypeId>,
}

/// A field (for Record) or variant (for Enum).
#[derive(Debug, Clone)]
pub struct InferredMember<'src> {
    pub name: &'src str,
    pub ty: TypeId,
}

// ─────────────────────────────────────────────────────────────────────────────
// Cardinality
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Cardinality {
    #[default]
    One,
    Optional,
    Star,
    Plus,
}

impl Cardinality {
    /// Join cardinalities when merging alternation branches.
    fn join(self, other: Cardinality) -> Cardinality {
        use Cardinality::*;
        match (self, other) {
            (One, One) => One,
            (One, Optional) | (Optional, One) | (Optional, Optional) => Optional,
            (Plus, Plus) => Plus,
            (One, Plus) | (Plus, One) => Plus,
            _ => Star,
        }
    }

    fn make_optional(self) -> Cardinality {
        use Cardinality::*;
        match self {
            One => Optional,
            Plus => Star,
            x => x,
        }
    }

    /// Multiply cardinalities (outer * inner).
    fn multiply(self, inner: Cardinality) -> Cardinality {
        use Cardinality::*;
        match (self, inner) {
            (One, x) => x,
            (x, One) => x,
            (Optional, Optional) => Optional,
            (Plus, Plus) => Plus,
            _ => Star,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Type shape for unification checking
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
enum TypeShape {
    Primitive(TypeId),
}

impl TypeShape {
    fn to_description(&self) -> TypeDescription {
        match self {
            TypeShape::Primitive(TYPE_NODE) => TypeDescription::Node,
            TypeShape::Primitive(TYPE_STR) => TypeDescription::String,
            TypeShape::Primitive(_) => TypeDescription::Node,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Field tracking within a scope
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct FieldInfo {
    base_type: TypeId,
    shape: TypeShape,
    cardinality: Cardinality,
    branch_count: usize,
    spans: Vec<TextRange>,
}

#[derive(Debug, Clone, Default)]
struct ScopeInfo<'src> {
    fields: IndexMap<&'src str, FieldInfo>,
    #[allow(dead_code)] // May be used for future enum variant tracking
    variants: IndexMap<&'src str, ScopeInfo<'src>>,
    #[allow(dead_code)]
    has_variants: bool,
}

impl<'src> ScopeInfo<'src> {
    fn add_field(
        &mut self,
        name: &'src str,
        base_type: TypeId,
        cardinality: Cardinality,
        span: TextRange,
    ) {
        let shape = TypeShape::Primitive(base_type);
        if let Some(existing) = self.fields.get_mut(name) {
            existing.cardinality = existing.cardinality.join(cardinality);
            existing.branch_count += 1;
            existing.spans.push(span);
        } else {
            self.fields.insert(
                name,
                FieldInfo {
                    base_type,
                    shape,
                    cardinality,
                    branch_count: 1,
                    spans: vec![span],
                },
            );
        }
    }

    fn merge_from(&mut self, other: ScopeInfo<'src>) -> Vec<MergeError<'src>> {
        let mut errors = Vec::new();

        for (name, other_info) in other.fields {
            if let Some(existing) = self.fields.get_mut(name) {
                if existing.shape != other_info.shape {
                    errors.push(MergeError {
                        field: name,
                        shapes: vec![existing.shape.clone(), other_info.shape.clone()],
                        spans: existing
                            .spans
                            .iter()
                            .chain(&other_info.spans)
                            .cloned()
                            .collect(),
                    });
                }
                existing.cardinality = existing.cardinality.join(other_info.cardinality);
                existing.branch_count += other_info.branch_count;
                existing.spans.extend(other_info.spans);
            } else {
                self.fields.insert(name, other_info);
            }
        }

        errors
    }

    fn apply_optionality(&mut self, total_branches: usize) {
        for info in self.fields.values_mut() {
            if info.branch_count < total_branches {
                info.cardinality = info.cardinality.make_optional();
            }
        }
    }

    #[allow(dead_code)] // May be useful for future scope analysis
    fn is_empty(&self) -> bool {
        self.fields.is_empty() && self.variants.is_empty()
    }
}

#[derive(Debug)]
struct MergeError<'src> {
    field: &'src str,
    shapes: Vec<TypeShape>,
    spans: Vec<TextRange>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Inference result from expression
// ─────────────────────────────────────────────────────────────────────────────

/// What an expression produces when evaluated.
#[derive(Debug, Clone)]
struct ExprResult {
    /// Base type (before cardinality wrapping).
    base_type: TypeId,
    /// Cardinality modifier.
    cardinality: Cardinality,
    /// True if this result represents a meaningful type (not just default Node).
    /// Used to distinguish QIS array results from simple uncaptured expressions.
    is_meaningful: bool,
}

impl ExprResult {
    fn node() -> Self {
        Self {
            base_type: TYPE_NODE,
            cardinality: Cardinality::One,
            is_meaningful: false,
        }
    }

    fn void() -> Self {
        Self {
            base_type: TYPE_VOID,
            cardinality: Cardinality::One,
            is_meaningful: false,
        }
    }

    fn meaningful(type_id: TypeId) -> Self {
        Self {
            base_type: type_id,
            cardinality: Cardinality::One,
            is_meaningful: true,
        }
    }

    /// Type is known but doesn't contribute to definition result (e.g., opaque references).
    fn opaque(type_id: TypeId) -> Self {
        Self {
            base_type: type_id,
            cardinality: Cardinality::One,
            is_meaningful: false,
        }
    }

    fn with_cardinality(mut self, card: Cardinality) -> Self {
        self.cardinality = card;
        self
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Inference context
// ─────────────────────────────────────────────────────────────────────────────

struct InferenceContext<'src> {
    source: &'src str,
    qis_triggers: HashSet<ast::QuantifiedExpr>,
    type_defs: Vec<InferredTypeDef<'src>>,
    next_type_id: TypeId,
    diagnostics: Diagnostics,
    errors: Vec<UnificationError<'src>>,
    current_def_name: &'src str,
    /// Map from definition name to its computed type.
    definition_types: HashMap<&'src str, TypeId>,
}

impl<'src> InferenceContext<'src> {
    fn new(source: &'src str, qis_triggers: HashSet<ast::QuantifiedExpr>) -> Self {
        Self {
            source,
            qis_triggers,
            type_defs: Vec::new(),
            next_type_id: 3, // 0=void, 1=node, 2=str
            diagnostics: Diagnostics::default(),
            errors: Vec::new(),
            current_def_name: "",
            definition_types: HashMap::new(),
        }
    }

    fn alloc_type_id(&mut self) -> TypeId {
        let id = self.next_type_id;
        self.next_type_id += 1;
        id
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Definition inference
    // ─────────────────────────────────────────────────────────────────────────

    fn infer_definition(&mut self, def_name: &'src str, body: &Expr) -> TypeId {
        self.current_def_name = def_name;

        let mut scope = ScopeInfo::default();
        let mut merge_errors = Vec::new();

        // Special case: tagged alternation at definition root creates enum
        if let Expr::AltExpr(alt) = body
            && alt.kind() == AltKind::Tagged
        {
            return self.infer_tagged_alternation_as_enum(def_name, alt, &mut merge_errors);
        }

        // General case: infer expression and collect captures into scope
        let result = self.infer_expr(body, &mut scope, Cardinality::One, &mut merge_errors);

        self.report_merge_errors(&merge_errors);

        // Build result type from scope
        if !scope.fields.is_empty() {
            self.create_struct_type(def_name, &scope)
        } else if result.is_meaningful {
            // QIS or other expressions that produce a meaningful type without populating scope
            result.base_type
        } else {
            TYPE_VOID
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Expression inference
    // ─────────────────────────────────────────────────────────────────────────

    fn infer_expr(
        &mut self,
        expr: &Expr,
        scope: &mut ScopeInfo<'src>,
        outer_card: Cardinality,
        errors: &mut Vec<MergeError<'src>>,
    ) -> ExprResult {
        match expr {
            Expr::CapturedExpr(c) => self.infer_captured(c, scope, outer_card, errors),
            Expr::QuantifiedExpr(q) => self.infer_quantified(q, scope, outer_card, errors),
            Expr::SeqExpr(s) => self.infer_sequence(s, scope, outer_card, errors),
            Expr::AltExpr(a) => self.infer_alternation(a, scope, outer_card, errors),
            Expr::NamedNode(n) => self.infer_named_node(n, scope, outer_card, errors),
            Expr::FieldExpr(f) => self.infer_field_expr(f, scope, outer_card, errors),
            Expr::Ref(r) => self.infer_ref(r),
            Expr::AnonymousNode(_) => ExprResult::node(),
        }
    }

    fn infer_captured(
        &mut self,
        c: &ast::CapturedExpr,
        scope: &mut ScopeInfo<'src>,
        outer_card: Cardinality,
        errors: &mut Vec<MergeError<'src>>,
    ) -> ExprResult {
        let capture_name = c.name().map(|t| token_src(&t, self.source)).unwrap_or("_");
        let span = c.text_range();
        let has_string_annotation = c
            .type_annotation()
            .and_then(|t| t.name())
            .is_some_and(|n| n.text() == "string");

        let Some(inner) = c.inner() else {
            return ExprResult::node();
        };

        // Check if inner is a scope container (seq/alt)
        let is_scope_container = matches!(inner, Expr::SeqExpr(_) | Expr::AltExpr(_));

        if is_scope_container {
            // Captured scope container: creates nested type
            let nested_type = self.infer_captured_container(capture_name, &inner, errors);
            let result = ExprResult::meaningful(nested_type);
            let effective_card = outer_card.multiply(result.cardinality);
            scope.add_field(capture_name, result.base_type, effective_card, span);
            result
        } else {
            // Simple capture: just capture the result
            let result = self.infer_expr(&inner, scope, outer_card, errors);
            let base_type = if has_string_annotation {
                TYPE_STR
            } else {
                result.base_type
            };
            let effective_card = outer_card.multiply(result.cardinality);
            scope.add_field(capture_name, base_type, effective_card, span);
            ExprResult::meaningful(base_type).with_cardinality(result.cardinality)
        }
    }

    fn infer_captured_container(
        &mut self,
        _capture_name: &'src str,
        inner: &Expr,
        errors: &mut Vec<MergeError<'src>>,
    ) -> TypeId {
        match inner {
            Expr::SeqExpr(s) => {
                let mut nested_scope = ScopeInfo::default();
                for child in s.children() {
                    self.infer_expr(&child, &mut nested_scope, Cardinality::One, errors);
                }
                let type_name = self.generate_scope_name();
                self.create_struct_type(type_name, &nested_scope)
            }
            Expr::AltExpr(a) => {
                if a.kind() == AltKind::Tagged {
                    // Captured tagged alternation → Enum
                    let type_name = self.generate_scope_name();
                    self.infer_tagged_alternation_as_enum(type_name, a, errors)
                } else {
                    // Captured untagged alternation → Struct with merged fields
                    let mut nested_scope = ScopeInfo::default();
                    self.infer_untagged_alternation(a, &mut nested_scope, Cardinality::One, errors);
                    let type_name = self.generate_scope_name();
                    self.create_struct_type(type_name, &nested_scope)
                }
            }
            _ => {
                // Not a container - shouldn't reach here
                TYPE_NODE
            }
        }
    }

    fn infer_quantified(
        &mut self,
        q: &ast::QuantifiedExpr,
        scope: &mut ScopeInfo<'src>,
        outer_card: Cardinality,
        errors: &mut Vec<MergeError<'src>>,
    ) -> ExprResult {
        let Some(inner) = q.inner() else {
            return ExprResult::node();
        };

        let quant_card = self.quantifier_cardinality(q);
        let is_qis = self.qis_triggers.contains(q);

        if is_qis {
            // QIS: create implicit scope for multiple captures
            let mut nested_scope = ScopeInfo::default();
            self.infer_expr(&inner, &mut nested_scope, Cardinality::One, errors);

            let element_type = if !nested_scope.fields.is_empty() {
                let type_name = self.generate_scope_name();
                self.create_struct_type(type_name, &nested_scope)
            } else {
                TYPE_NODE
            };

            // Wrap with array type - this is a meaningful result
            let array_type = self.wrap_with_cardinality(element_type, quant_card);
            ExprResult::meaningful(array_type)
        } else {
            // No QIS: captures propagate with multiplied cardinality
            let combined_card = outer_card.multiply(quant_card);
            let result = self.infer_expr(&inner, scope, combined_card, errors);
            // Return result with quantifier's cardinality so captured quantifiers work correctly
            ExprResult {
                base_type: result.base_type,
                cardinality: quant_card.multiply(result.cardinality),
                is_meaningful: result.is_meaningful,
            }
        }
    }

    fn infer_sequence(
        &mut self,
        s: &ast::SeqExpr,
        scope: &mut ScopeInfo<'src>,
        outer_card: Cardinality,
        errors: &mut Vec<MergeError<'src>>,
    ) -> ExprResult {
        // Uncaptured sequence: captures propagate to parent scope
        let mut last_result = ExprResult::void();
        for child in s.children() {
            last_result = self.infer_expr(&child, scope, outer_card, errors);
        }
        last_result
    }

    fn infer_alternation(
        &mut self,
        a: &ast::AltExpr,
        scope: &mut ScopeInfo<'src>,
        outer_card: Cardinality,
        errors: &mut Vec<MergeError<'src>>,
    ) -> ExprResult {
        // Uncaptured alternation (tagged or untagged): captures propagate with optionality
        self.infer_untagged_alternation(a, scope, outer_card, errors)
    }

    fn infer_untagged_alternation(
        &mut self,
        a: &ast::AltExpr,
        scope: &mut ScopeInfo<'src>,
        outer_card: Cardinality,
        errors: &mut Vec<MergeError<'src>>,
    ) -> ExprResult {
        let branches: Vec<_> = a.branches().collect();
        let total_branches = branches.len();

        if total_branches == 0 {
            return ExprResult::void();
        }

        let mut merged_scope = ScopeInfo::default();

        for branch in &branches {
            let Some(body) = branch.body() else {
                continue;
            };
            let mut branch_scope = ScopeInfo::default();
            self.infer_expr(&body, &mut branch_scope, outer_card, errors);
            errors.extend(merged_scope.merge_from(branch_scope));
        }

        // Apply optionality for fields not present in all branches
        merged_scope.apply_optionality(total_branches);

        // Merge into parent scope
        errors.extend(scope.merge_from(merged_scope));

        ExprResult::node()
    }

    fn infer_tagged_alternation_as_enum(
        &mut self,
        type_name: &'src str,
        a: &ast::AltExpr,
        errors: &mut Vec<MergeError<'src>>,
    ) -> TypeId {
        let mut variants = IndexMap::new();

        for branch in a.branches() {
            let tag = branch
                .label()
                .map(|t| token_src(&t, self.source))
                .unwrap_or("_");
            let Some(body) = branch.body() else {
                variants.insert(tag, ScopeInfo::default());
                continue;
            };

            let mut variant_scope = ScopeInfo::default();
            self.infer_expr(&body, &mut variant_scope, Cardinality::One, errors);
            variants.insert(tag, variant_scope);
        }

        self.create_enum_type_from_variants(type_name, &variants)
    }

    fn infer_named_node(
        &mut self,
        n: &ast::NamedNode,
        scope: &mut ScopeInfo<'src>,
        outer_card: Cardinality,
        errors: &mut Vec<MergeError<'src>>,
    ) -> ExprResult {
        // Named nodes have children - recurse into them
        for child in n.children() {
            self.infer_expr(&child, scope, outer_card, errors);
        }
        ExprResult::node()
    }

    fn infer_field_expr(
        &mut self,
        f: &ast::FieldExpr,
        scope: &mut ScopeInfo<'src>,
        outer_card: Cardinality,
        errors: &mut Vec<MergeError<'src>>,
    ) -> ExprResult {
        // Field constraint (name: expr) - just recurse
        if let Some(value) = f.value() {
            return self.infer_expr(&value, scope, outer_card, errors);
        }
        ExprResult::node()
    }

    fn infer_ref(&self, r: &ast::Ref) -> ExprResult {
        // References are opaque - captures don't propagate from referenced definition.
        // Return the type (for use when captured) but mark as not meaningful
        // so uncaptured refs don't affect definition's result type.
        let ref_name = r.name().map(|t| t.text().to_string());
        if let Some(name) = ref_name
            && let Some(&type_id) = self.definition_types.get(name.as_str())
        {
            return ExprResult::opaque(type_id);
        }
        ExprResult::node()
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Helpers
    // ─────────────────────────────────────────────────────────────────────────

    fn quantifier_cardinality(&self, q: &ast::QuantifiedExpr) -> Cardinality {
        let Some(op) = q.operator() else {
            return Cardinality::One;
        };
        use crate::parser::cst::SyntaxKind;
        match op.kind() {
            SyntaxKind::Star | SyntaxKind::StarQuestion => Cardinality::Star,
            SyntaxKind::Plus | SyntaxKind::PlusQuestion => Cardinality::Plus,
            SyntaxKind::Question | SyntaxKind::QuestionQuestion => Cardinality::Optional,
            _ => Cardinality::One,
        }
    }

    fn generate_scope_name(&self) -> &'src str {
        let name = format!("{}Scope{}", self.current_def_name, self.next_type_id);
        Box::leak(name.into_boxed_str())
    }

    fn create_struct_type(&mut self, name: &'src str, scope: &ScopeInfo<'src>) -> TypeId {
        let members: Vec<_> = scope
            .fields
            .iter()
            .map(|(field_name, info)| {
                let member_type = self.wrap_with_cardinality(info.base_type, info.cardinality);
                InferredMember {
                    name: field_name,
                    ty: member_type,
                }
            })
            .collect();

        let type_id = self.alloc_type_id();

        self.type_defs.push(InferredTypeDef {
            kind: TypeKind::Record,
            name: Some(name),
            members,
            inner_type: None,
        });

        type_id
    }

    fn create_enum_type_from_variants(
        &mut self,
        name: &'src str,
        variants: &IndexMap<&'src str, ScopeInfo<'src>>,
    ) -> TypeId {
        let mut members = Vec::new();

        for (tag, variant_scope) in variants {
            let variant_type = if variant_scope.fields.is_empty() {
                TYPE_VOID
            } else if variant_scope.fields.len() == 1 {
                // Single-capture variant: flatten (ADR-0007)
                let (_, info) = variant_scope.fields.iter().next().unwrap();
                self.wrap_with_cardinality(info.base_type, info.cardinality)
            } else {
                let variant_name = self.generate_scope_name();
                self.create_struct_type(variant_name, variant_scope)
            };
            members.push(InferredMember {
                name: tag,
                ty: variant_type,
            });
        }

        let type_id = self.alloc_type_id();

        self.type_defs.push(InferredTypeDef {
            kind: TypeKind::Enum,
            name: Some(name),
            members,
            inner_type: None,
        });

        type_id
    }

    fn wrap_with_cardinality(&mut self, base: TypeId, card: Cardinality) -> TypeId {
        match card {
            Cardinality::One => base,
            Cardinality::Optional => {
                let type_id = self.alloc_type_id();
                self.type_defs.push(InferredTypeDef {
                    kind: TypeKind::Optional,
                    name: None,
                    members: Vec::new(),
                    inner_type: Some(base),
                });
                type_id
            }
            Cardinality::Star => {
                let type_id = self.alloc_type_id();
                self.type_defs.push(InferredTypeDef {
                    kind: TypeKind::ArrayStar,
                    name: None,
                    members: Vec::new(),
                    inner_type: Some(base),
                });
                type_id
            }
            Cardinality::Plus => {
                let type_id = self.alloc_type_id();
                self.type_defs.push(InferredTypeDef {
                    kind: TypeKind::ArrayPlus,
                    name: None,
                    members: Vec::new(),
                    inner_type: Some(base),
                });
                type_id
            }
        }
    }

    fn report_merge_errors(&mut self, merge_errors: &[MergeError<'src>]) {
        for err in merge_errors {
            let types_str = err
                .shapes
                .iter()
                .map(|s| s.to_description().to_string())
                .collect::<Vec<_>>()
                .join(" vs ");

            let primary_span = err.spans.first().copied().unwrap_or_default();
            let mut builder = self
                .diagnostics
                .report(DiagnosticKind::IncompatibleTypes, primary_span)
                .message(types_str);

            for span in err.spans.iter().skip(1) {
                builder = builder.related_to("also captured here", *span);
            }
            builder
                .hint(format!(
                    "capture `{}` has incompatible types across branches",
                    err.field
                ))
                .emit();

            self.errors.push(UnificationError {
                field: err.field,
                definition: self.current_def_name,
                types_found: err.shapes.iter().map(|s| s.to_description()).collect(),
                spans: err.spans.clone(),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Query integration
// ─────────────────────────────────────────────────────────────────────────────

impl<'a> Query<'a> {
    /// Run type inference on the query AST.
    pub(super) fn infer_types(&mut self) {
        // Collect QIS triggers upfront to avoid borrowing issues
        let qis_triggers: HashSet<_> = self.qis_triggers.keys().cloned().collect();
        let sorted = self.topological_sort_definitions_ast();

        let mut ctx = InferenceContext::new(self.source, qis_triggers);

        // Process definitions in dependency order
        for (name, body) in &sorted {
            let type_id = ctx.infer_definition(name, body);
            ctx.definition_types.insert(name, type_id);
        }

        // Preserve symbol table order for entrypoints
        for (name, _) in &sorted {
            if let Some(&type_id) = ctx.definition_types.get(name) {
                self.type_info.entrypoint_types.insert(*name, type_id);
            }
        }
        self.type_info.type_defs = ctx.type_defs;
        self.type_info.diagnostics = ctx.diagnostics;
        self.type_info.errors = ctx.errors;
    }

    /// Topologically sort definitions for processing order.
    fn topological_sort_definitions_ast(&self) -> Vec<(&'a str, ast::Expr)> {
        use std::collections::{HashSet, VecDeque};

        let definitions: Vec<_> = self
            .symbol_table
            .iter()
            .map(|(&name, body)| (name, body.clone()))
            .collect();
        let def_names: HashSet<&str> = definitions.iter().map(|(name, _)| *name).collect();

        // Build dependency graph from AST references
        let mut deps: HashMap<&str, Vec<&str>> = HashMap::new();
        for (name, body) in &definitions {
            let refs = Self::collect_ast_references(body, &def_names);
            deps.insert(name, refs);
        }

        // Kahn's algorithm
        let mut in_degree: HashMap<&str, usize> = HashMap::new();
        for (name, _) in &definitions {
            in_degree.insert(name, 0);
        }
        for refs in deps.values() {
            for &dep in refs {
                *in_degree.entry(dep).or_insert(0) += 1;
            }
        }

        let mut zero_degree: Vec<&str> = in_degree
            .iter()
            .filter(|(_, deg)| **deg == 0)
            .map(|(&name, _)| name)
            .collect();
        zero_degree.sort();
        let mut queue: VecDeque<&str> = zero_degree.into_iter().collect();

        let mut sorted_names = Vec::new();
        while let Some(name) = queue.pop_front() {
            sorted_names.push(name);
            if let Some(refs) = deps.get(name) {
                for &dep in refs {
                    if let Some(deg) = in_degree.get_mut(dep) {
                        *deg = deg.saturating_sub(1);
                        if *deg == 0 {
                            queue.push_back(dep);
                        }
                    }
                }
            }
        }

        // Reverse so dependencies come first
        sorted_names.reverse();

        // Add any remaining (cyclic) definitions
        for (name, _) in &definitions {
            if !sorted_names.contains(name) {
                sorted_names.push(name);
            }
        }

        // Build result with bodies
        sorted_names
            .into_iter()
            .filter_map(|name| self.symbol_table.get(name).map(|body| (name, body.clone())))
            .collect()
    }

    /// Collect references from an AST expression.
    fn collect_ast_references<'b>(expr: &Expr, def_names: &HashSet<&'b str>) -> Vec<&'b str> {
        let mut refs = Vec::new();
        Self::collect_ast_references_impl(expr, def_names, &mut refs);
        refs
    }

    fn collect_ast_references_impl<'b>(
        expr: &Expr,
        def_names: &HashSet<&'b str>,
        refs: &mut Vec<&'b str>,
    ) {
        match expr {
            Expr::Ref(r) => {
                if let Some(name_token) = r.name() {
                    let name = name_token.text();
                    if def_names.contains(name) && !refs.contains(&name) {
                        // Find the actual &'b str from the set
                        if let Some(&found) = def_names.iter().find(|&&n| n == name) {
                            refs.push(found);
                        }
                    }
                }
            }
            _ => {
                for child in expr.children() {
                    Self::collect_ast_references_impl(&child, def_names, refs);
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Display and helpers
// ─────────────────────────────────────────────────────────────────────────────

impl TypeInferenceResult<'_> {
    pub fn dump(&self) -> String {
        let mut out = String::new();

        out.push_str("=== Entrypoints ===\n");
        for (name, type_id) in &self.entrypoint_types {
            out.push_str(&format!("{} → {}\n", name, format_type_id(*type_id)));
        }

        if !self.type_defs.is_empty() {
            out.push_str("\n=== Types ===\n");
            for (idx, def) in self.type_defs.iter().enumerate() {
                let type_id = 3 + idx as TypeId;
                let name = def.name.unwrap_or("<anon>");
                match def.kind {
                    TypeKind::Record => {
                        out.push_str(&format!("T{}: Record {} {{\n", type_id, name));
                        for member in &def.members {
                            out.push_str(&format!(
                                "    {}: {}\n",
                                member.name,
                                format_type_id(member.ty)
                            ));
                        }
                        out.push_str("}\n");
                    }
                    TypeKind::Enum => {
                        out.push_str(&format!("T{}: Enum {} {{\n", type_id, name));
                        for member in &def.members {
                            out.push_str(&format!(
                                "    {}: {}\n",
                                member.name,
                                format_type_id(member.ty)
                            ));
                        }
                        out.push_str("}\n");
                    }
                    TypeKind::Optional => {
                        let inner = def.inner_type.map(format_type_id).unwrap_or_default();
                        out.push_str(&format!("T{}: Optional {} → {}\n", type_id, name, inner));
                    }
                    TypeKind::ArrayStar => {
                        let inner = def.inner_type.map(format_type_id).unwrap_or_default();
                        out.push_str(&format!("T{}: ArrayStar {} → {}\n", type_id, name, inner));
                    }
                    TypeKind::ArrayPlus => {
                        let inner = def.inner_type.map(format_type_id).unwrap_or_default();
                        out.push_str(&format!("T{}: ArrayPlus {} → {}\n", type_id, name, inner));
                    }
                }
            }
        }

        if !self.errors.is_empty() {
            out.push_str("\n=== Errors ===\n");
            for err in &self.errors {
                let types = err
                    .types_found
                    .iter()
                    .map(|t| t.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push_str(&format!(
                    "field `{}` in `{}`: incompatible types [{}]\n",
                    err.field, err.definition, types
                ));
            }
        }

        out
    }

    pub fn dump_diagnostics(&self, source: &str) -> String {
        self.diagnostics.render_filtered(source)
    }

    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

fn format_type_id(id: TypeId) -> String {
    match id {
        TYPE_VOID => "Void".to_string(),
        TYPE_NODE => "Node".to_string(),
        TYPE_STR => "String".to_string(),
        _ => format!("T{}", id),
    }
}
