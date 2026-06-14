//! Capture effects handling for query compilation.
//!
//! Manages the construction and propagation of capture effects (Node/Text + Set)
//! through the compilation pipeline.

use std::collections::HashSet;

use crate::analyze::type_check::{
    CaptureMechanism, TypeContext, TypeId, TypeShape, capture_mechanism,
};
use crate::bytecode::EffectIR;
use crate::parser::ast::{self, Expr};
use plotnik_bytecode::EffectOpcode;

use super::Compiler;

/// Capture effects to attach to match instructions.
///
/// Instead of emitting separate epsilon transitions for wrapper effects,
/// these effects are propagated through the compilation chain and attached
/// directly to match instructions.
///
/// For sequences `{a b c}`:
/// - `pre` effects go on the first item (entry)
/// - `post` effects go on the last item (exit)
///
/// For tagged alternations `[A: body]`:
/// - `pre` contains `Enum(variant)` for branch entry
/// - `post` contains `EndEnum` for branch exit
#[derive(Clone, Default)]
pub struct CaptureEffects {
    /// Effects to place as pre_effects on the entry instruction.
    /// Used for: Enum(variant) in tagged alternations.
    pub pre: Vec<EffectIR>,
    /// Effects to place as post_effects on the exit instruction.
    /// Typically: [Node/Text, Set(member)], [Push], or [EndEnum].
    pub post: Vec<EffectIR>,
}

impl CaptureEffects {
    /// Create with explicit pre and post effects.
    pub fn new(pre: Vec<EffectIR>, post: Vec<EffectIR>) -> Self {
        Self { pre, post }
    }

    /// Create with only pre effects.
    pub fn new_pre(pre: Vec<EffectIR>) -> Self {
        Self { pre, post: vec![] }
    }

    /// Create with only post effects.
    pub fn new_post(post: Vec<EffectIR>) -> Self {
        Self { pre: vec![], post }
    }

    /// Add an inner scope (opens after existing scopes, closes before them).
    ///
    /// Use for: Obj/EndObj, Enum/EndEnum, Arr/EndArr, SuppressBegin/SuppressEnd
    ///
    /// Given existing `pre=[A_Open]`, `post=[A_Close]`, adding inner scope B:
    /// - Result: `pre=[A_Open, B_Open]`, `post=[B_Close, A_Close]`
    /// - Execution: A opens -> B opens -> match -> B closes -> A closes
    pub fn nest_scope(mut self, open: EffectIR, close: EffectIR) -> Self {
        assert!(
            matches!(
                open.opcode,
                EffectOpcode::Obj
                    | EffectOpcode::Enum
                    | EffectOpcode::Arr
                    | EffectOpcode::SuppressBegin
            ),
            "nest_scope expects scope-opening effect, got {:?}",
            open.opcode
        );
        assert!(
            matches!(
                close.opcode,
                EffectOpcode::EndObj
                    | EffectOpcode::EndEnum
                    | EffectOpcode::EndArr
                    | EffectOpcode::SuppressEnd
            ),
            "nest_scope expects scope-closing effect, got {:?}",
            close.opcode
        );
        self.pre.push(open);
        self.post.insert(0, close);
        self
    }

    /// Add pre-match value effects (run after all scopes open).
    ///
    /// Use for: Null+Set injection in untagged alternations
    ///
    /// Given `pre=[Scope_Open]`, adding value effects:
    /// - Result: `pre=[Scope_Open, Value1, Value2]`
    pub fn with_pre_values(mut self, effects: Vec<EffectIR>) -> Self {
        self.pre.extend(effects);
        self
    }

    /// Add post-match value effects (run before any scope closes).
    ///
    /// Use for: Node/Text+Set capture effects, Push for arrays
    ///
    /// Given `post=[Scope_Close]`, adding value effects:
    /// - Result: `post=[Value1, Value2, Scope_Close]`
    pub fn with_post_values(mut self, effects: Vec<EffectIR>) -> Self {
        self.post.splice(0..0, effects);
        self
    }
}

impl Compiler<'_> {
    /// Build capture effects (Node/Text + Set) based on capture type.
    pub(super) fn build_capture_effects(
        &self,
        cap: &ast::CapturedExpr,
        inner: Option<&Expr>,
    ) -> Vec<EffectIR> {
        let mut effects = Vec::with_capacity(2);

        // Only the `Node` mechanism captures the matched node/text directly. Every
        // other mechanism (struct scope, pass-through ref/enum/forward, array)
        // produces its value via EndObj/EndEnum/EndArr/Call, so the capture itself
        // emits no Node/Text. A bare capture (`@x` with no inner) is a Node.
        let is_node_mechanism = inner.is_none_or(|i| {
            capture_mechanism(i, self.ctx.type_ctx, self.ctx.interner) == CaptureMechanism::Node
        });
        if is_node_mechanism {
            let effect = if cap.has_string_annotation() {
                EffectIR::text()
            } else {
                EffectIR::node()
            };
            effects.push(effect);
        }

        // Add Set effect if we have a capture name.
        // Always look up in the current scope - bubble captures don't create new scopes,
        // so all fields (including nested bubble captures) reference the same root struct.
        if let Some(name_token) = cap.name() {
            let capture_name = &name_token.text()[1..]; // Strip @ prefix
            let member_ref = self.lookup_member_in_scope(capture_name);
            if let Some(member_ref) = member_ref {
                effects.push(EffectIR::with_member(EffectOpcode::Set, member_ref));
            }
        }

        effects
    }

    /// Check if a quantifier body needs Node effect before Push.
    ///
    /// For scalar array elements (Node/String types), we need [Node/Text, Push]
    /// to capture the matched node value.
    /// For structured elements (Struct/Enum), EndObj/EndEnum provides the value.
    /// For refs returning structured types, Call provides the value.
    pub(super) fn quantifier_needs_node_for_push(&self, expr: &Expr) -> bool {
        let Expr::QuantifiedExpr(quant) = expr else {
            return true;
        };
        let Some(inner) = quant.inner() else {
            return true;
        };

        // Refs returning structured types don't need Node
        if self.is_ref_returning_structured(&inner) {
            return false;
        }

        // Check the actual inferred type, not syntax
        let Some(info) = self.ctx.type_ctx.get_term_info(&inner) else {
            return true;
        };

        // If type is Struct or Enum, EndObj/EndEnum produces the value
        // Otherwise (Node, String, Void, etc.), we need Node effect
        !info
            .flow
            .type_id()
            .and_then(|id| self.ctx.type_ctx.get_type(id))
            .is_some_and(|shape| matches!(shape, TypeShape::Struct(_) | TypeShape::Enum(_)))
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
            .and_then(|name| self.ctx.type_ctx.get_def_id(self.ctx.interner, name.text()))
            .and_then(|def_id| self.ctx.type_ctx.get_def_type(def_id))
            .and_then(|def_type| self.ctx.type_ctx.get_type(def_type))
            .is_some_and(|shape| {
                matches!(
                    shape,
                    TypeShape::Struct(_) | TypeShape::Enum(_) | TypeShape::Array { .. }
                )
            })
    }

    /// Collect all capture names from an expression recursively.
    pub(super) fn collect_captures(expr: &Expr) -> HashSet<String> {
        fn collect(expr: &Expr, names: &mut HashSet<String>) {
            if let Expr::CapturedExpr(cap) = expr
                && let Some(name) = cap.name()
            {
                names.insert(name.text()[1..].to_string()); // Strip @ prefix
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
/// Returns true when the inner expression produces a Struct type (bubbling fields).
/// This includes:
/// - Sequences/alternations with captures: `{(a) @x (b) @y}*`
/// - Named nodes with bubble captures: `(node (child) @x)*`
///
/// Enums use Enum/EndEnum instead (handled separately).
pub fn check_needs_struct_wrapper(inner: &Expr, type_ctx: &TypeContext) -> bool {
    let Some(info) = type_ctx.get_term_info(inner) else {
        return false;
    };

    // Must be a bubble (fields flow to parent scope)
    if !info.flow.is_bubble() {
        return false;
    }

    // Check the actual type - if it's a Struct, we need Obj/EndObj wrapper
    info.flow
        .type_id()
        .and_then(|id| type_ctx.get_type(id))
        .is_some_and(|shape| matches!(shape, TypeShape::Struct(_)))
}

/// Get row type ID for array element scoping.
pub fn get_row_type_id(inner: &Expr, type_ctx: &TypeContext) -> Option<TypeId> {
    type_ctx
        .get_term_info(inner)
        .and_then(|info| info.flow.type_id())
}
