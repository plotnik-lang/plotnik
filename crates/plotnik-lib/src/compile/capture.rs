//! Capture effects handling for query compilation.
//!
//! Manages the construction and propagation of capture effects (Node/Text + Set)
//! through the compilation pipeline.

use std::collections::HashSet;

use crate::bytecode::ir::EffectIR;
use crate::bytecode::EffectOpcode;
use crate::parser::ast::{self, Expr};
use crate::analyze::type_check::{TypeContext, TypeId, TypeShape};

use super::navigation::{inner_creates_scope, is_star_or_plus_quantifier};
use super::Compiler;

/// Capture effects to attach to the innermost match instruction.
///
/// Instead of emitting a separate epsilon transition for capture effects,
/// these effects are propagated through the compilation chain and attached
/// directly to the match instruction that captures the node.
#[derive(Clone, Default)]
pub struct CaptureEffects {
    /// Effects to place as post_effects on the matching instruction.
    /// Typically: [Node/Text, Set(member)] or [Node/Text, Push]
    pub post: Vec<EffectIR>,
}

impl Compiler<'_> {
    /// Build capture effects (Node/Text + Set) based on capture type.
    pub(super) fn build_capture_effects(
        &self,
        cap: &ast::CapturedExpr,
        inner: Option<&Expr>,
    ) -> Vec<EffectIR> {
        let mut effects = Vec::with_capacity(2);

        // Skip Node/Text when the value comes from somewhere other than matched_node:
        // 1. Refs returning structured types (Call leaves result pending)
        // 2. Scope-creating expressions (Seq/Alt) producing structured types (EndObj/EndEnum)
        // 3. Array captures (EndArr produces value)
        let is_structured_ref = inner.is_some_and(|i| self.is_ref_returning_structured(i));
        let is_array = is_star_or_plus_quantifier(inner);

        // Check if inner is a scope-creating expression (SeqExpr/AltExpr) that produces
        // a structured type (Struct/Enum). Named nodes with bubble captures don't count -
        // they still need Node because we're capturing the matched node, not the struct.
        let creates_structured_scope = inner.is_some_and(|i| {
            inner_creates_scope(i)
                && self
                    .type_ctx
                    .get_term_info(i)
                    .and_then(|info| info.flow.type_id())
                    .and_then(|id| self.type_ctx.get_type(id))
                    .is_some_and(|shape| matches!(shape, TypeShape::Struct(_) | TypeShape::Enum(_)))
        });

        if !is_structured_ref && !creates_structured_scope && !is_array {
            let is_text = cap.type_annotation().is_some_and(|t| {
                t.name().is_some_and(|n| n.text() == "string")
            });
            let opcode = if is_text { EffectOpcode::Text } else { EffectOpcode::Node };
            effects.push(EffectIR::simple(opcode, 0));
        }

        // Add Set effect if we have a capture name.
        // Always look up in the current scope - bubble captures don't create new scopes,
        // so all fields (including nested bubble captures) reference the same root struct.
        if let Some(name_token) = cap.name() {
            let capture_name = name_token.text();
            let member_ref = self.lookup_member_in_scope(capture_name);
            if let Some(member_ref) = member_ref {
                effects.push(EffectIR::with_member(EffectOpcode::Set, member_ref));
            }
        }

        effects
    }

    /// Check if a quantifier body needs Node effect before Push.
    ///
    /// For scalar array elements (simple named nodes, not structs/enums/refs),
    /// we need [Node, Push] to capture the matched node value.
    /// For structured elements, EndObj/EndEnum/Call already provides the value.
    pub(super) fn quantifier_needs_node_for_push(&self, expr: &Expr) -> bool {
        if let Expr::QuantifiedExpr(quant) = expr
            && let Some(body) = quant.inner()
        {
            !inner_creates_scope(&body) && !self.is_ref_returning_structured(&body)
        } else {
            true
        }
    }

    /// Check if expr is (or wraps) a ref returning a structured type.
    ///
    /// For such refs, we skip the Node effect in captures - the Call leaves
    /// the structured result (Enum/Struct/Array) pending for Set to consume.
    pub(super) fn is_ref_returning_structured(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Ref(r) => self.ref_returns_structured(r),
            Expr::QuantifiedExpr(q) => q
                .inner()
                .is_some_and(|i| self.is_ref_returning_structured(&i)),
            Expr::CapturedExpr(c) => c
                .inner()
                .is_some_and(|i| self.is_ref_returning_structured(&i)),
            Expr::FieldExpr(f) => f
                .value()
                .is_some_and(|v| self.is_ref_returning_structured(&v)),
            _ => false,
        }
    }

    /// Check if a Ref points to a definition returning a structured type.
    ///
    /// All refs now use Call/Return. If the definition returns a structured type
    /// (Enum/Struct/Array), Return leaves that result pending for Set to consume.
    /// In this case, we skip emitting Node/Text effects in captures.
    fn ref_returns_structured(&self, r: &ast::Ref) -> bool {
        r.name()
            .and_then(|name| self.type_ctx.get_def_id(self.interner, name.text()))
            .and_then(|def_id| self.type_ctx.get_def_type(def_id))
            .and_then(|def_type| self.type_ctx.get_type(def_type))
            .is_some_and(|shape| {
                matches!(shape, TypeShape::Struct(_) | TypeShape::Enum(_) | TypeShape::Array { .. })
            })
    }

    /// Collect all capture names from an expression recursively.
    pub(super) fn collect_captures(expr: &Expr) -> HashSet<String> {
        fn collect(expr: &Expr, names: &mut HashSet<String>) {
            if let Expr::CapturedExpr(cap) = expr
                && let Some(name) = cap.name()
            {
                names.insert(name.text().to_string());
            }
            for child in expr.children() {
                collect(&child, names);
            }
        }
        let mut names = HashSet::new();
        collect(expr, &mut names);
        names
    }
}

/// Check if inner needs struct wrapper for array iterations.
///
/// Returns true when inner is a scope-creating expression (sequence/alternation)
/// that produces an untagged struct (not an enum). Enums use Enum/EndEnum instead.
pub fn check_needs_struct_wrapper(
    inner: &Expr,
    type_ctx: &TypeContext,
) -> bool {
    let inner_info = type_ctx.get_term_info(inner);
    let inner_creates_scope = inner_creates_scope(inner);
    let inner_is_untagged_bubble = inner_info.as_ref().is_some_and(|info| {
        if !info.flow.is_bubble() {
            return false;
        }
        let Some(type_id) = info.flow.type_id() else {
            return false;
        };
        let Some(shape) = type_ctx.get_type(type_id) else {
            return false;
        };
        matches!(shape, TypeShape::Struct(_))
    });
    inner_is_untagged_bubble && inner_creates_scope
}

/// Get row type ID for array element scoping.
pub fn get_row_type_id(inner: &Expr, type_ctx: &TypeContext) -> Option<TypeId> {
    type_ctx.get_term_info(inner).and_then(|info| info.flow.type_id())
}
