//! Capture effects handling for query compilation.
//!
//! Manages the construction and propagation of capture effects (Node + Set)
//! through the compilation pipeline.

use std::collections::HashSet;

use crate::analyze::type_check::{CaptureKind, TypeContext, TypeId, TypeShape};
use crate::bytecode::{EffectIR, Label};
use crate::parser::ast::{self, Pattern};
use plotnik_bytecode::{EffectKind, Nav};

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
/// For enum alternations `[A: body]`:
/// - `pre` contains `Enum(variant)` for branch entry
/// - `post` contains `EndEnum` for branch exit
#[derive(Clone, Default)]
pub struct CaptureEffects {
    /// Effects to place as pre_effects on the entry instruction.
    pub pre: Vec<EffectIR>,
    /// Effects to place as post_effects on the exit instruction.
    pub post: Vec<EffectIR>,
}

impl CaptureEffects {
    pub fn new(pre: Vec<EffectIR>, post: Vec<EffectIR>) -> Self {
        Self { pre, post }
    }

    pub fn new_pre(pre: Vec<EffectIR>) -> Self {
        Self { pre, post: vec![] }
    }

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
                open.kind(),
                EffectKind::ObjectOpen
                    | EffectKind::EnumOpen
                    | EffectKind::ArrayOpen
                    | EffectKind::SuppressBegin
            ),
            "nest_scope expects scope-opening effect, got {:?}",
            open.kind()
        );
        assert!(
            matches!(
                close.kind(),
                EffectKind::ObjectClose
                    | EffectKind::EnumClose
                    | EffectKind::ArrayClose
                    | EffectKind::SuppressEnd
            ),
            "nest_scope expects scope-closing effect, got {:?}",
            close.kind()
        );
        self.pre.push(open);
        self.post.insert(0, close);
        self
    }

    /// Add pre-match value effects (run after all scopes open).
    ///
    /// Use for: Null+Set injection in union alternations
    ///
    /// Given `pre=[Scope_Open]`, adding value effects:
    /// - Result: `pre=[Scope_Open, Value1, Value2]`
    pub fn with_pre_values(mut self, effects: Vec<EffectIR>) -> Self {
        self.pre.extend(effects);
        self
    }

    /// Add post-match value effects (run before any scope closes).
    ///
    /// Use for: Node+Set capture effects, Push for arrays
    ///
    /// Given `post=[Scope_Close]`, adding value effects:
    /// - Result: `post=[Value1, Value2, Scope_Close]`
    pub fn with_post_values(mut self, effects: Vec<EffectIR>) -> Self {
        self.post.splice(0..0, effects);
        self
    }
}

/// The backbone calling convention threaded through the `dispatch_pattern` family.
///
/// Bundles the three values every expression compiler needs: where the compiled
/// fragment continues (`exit`), the navigation it should apply to reach its first
/// candidate (`nav`, `None` meaning "use the form's default"), and the capture
/// effects that land on its innermost match/scope-close instruction (`capture`).
#[derive(Clone)]
pub(super) struct ExprCtx {
    pub exit: Label,
    pub nav: Option<Nav>,
    pub capture: CaptureEffects,
}

impl ExprCtx {
    pub(super) fn with_nav(exit: Label, nav: Option<Nav>) -> Self {
        Self {
            exit,
            nav,
            capture: CaptureEffects::default(),
        }
    }
}

impl Compiler<'_> {
    /// Build capture effects (Node + Set) for a capture whose inner was
    /// classified as `mechanism` (or `None` for a bare `@x`).
    ///
    /// The caller already runs [`capture_kind`] to dispatch, so it passes the
    /// result in rather than have this re-classify the same inner.
    pub(super) fn build_capture_effects(
        &self,
        cap: &ast::CapturedPattern,
        mechanism: Option<CaptureKind>,
    ) -> Vec<EffectIR> {
        let mut effects = Vec::with_capacity(2);

        // Only the `Node` mechanism captures the matched node directly. Every
        // other mechanism (struct scope, pass-through ref/enum/forward, array)
        // produces its value via EndObj/EndEnum/EndArr/Call, so the capture itself
        // emits no Node. A bare capture (`@x` with no inner) is a Node.
        let is_node_mechanism = mechanism.is_none_or(|m| m == CaptureKind::Node);
        if is_node_mechanism {
            effects.push(EffectIR::node());
        }

        // Add Set effect if we have a capture name.
        // Always look up in the current scope - bubble captures don't create new scopes,
        // so all fields (including nested bubble captures) reference the same root struct.
        if let Some(name_token) = cap.name() {
            let capture_name = &name_token.text()[1..];
            let member_ref = self.lookup_member_in_scope(capture_name);
            if let Some(member_ref) = member_ref {
                effects.push(EffectIR::with_member(EffectKind::Set, member_ref));
            }
        }

        effects
    }

    /// Check if a quantifier body needs Node effect before Push.
    ///
    /// For scalar array elements (Node type), we need [Node, Push]
    /// to capture the matched node value.
    /// For structured elements (Struct/Enum), EndObj/EndEnum provides the value.
    /// For refs returning structured types, Call provides the value.
    pub(super) fn quantifier_needs_node_for_push(&self, pattern: &Pattern) -> bool {
        let Pattern::QuantifiedPattern(quant) = pattern else {
            return true;
        };
        let Some(inner) = quant.inner() else {
            return true;
        };

        if self.is_ref_returning_structured(&inner) {
            return false;
        }

        // Check the actual inferred type, not syntax
        let Some(info) = self.ctx.type_ctx.term_info(&inner) else {
            return true;
        };

        !info
            .flow
            .type_id()
            .and_then(|id| self.ctx.type_ctx.type_shape(id))
            .is_some_and(|shape| matches!(shape, TypeShape::Struct(_) | TypeShape::Enum(_)))
    }

    /// Check if pattern is (or wraps) a ref returning a structured type.
    ///
    /// For such refs, we skip the Node effect in captures - the Call leaves
    /// the structured result (Enum/Struct/Array) pending for Set to consume.
    pub(super) fn is_ref_returning_structured(&self, pattern: &Pattern) -> bool {
        match pattern {
            Pattern::Ref(r) => self.ref_returns_structured(r),
            Pattern::QuantifiedPattern(q) => q
                .inner()
                .is_some_and(|i| self.is_ref_returning_structured(&i)),
            Pattern::CapturedPattern(c) => c
                .inner()
                .is_some_and(|i| self.is_ref_returning_structured(&i)),
            Pattern::FieldPattern(f) => f
                .value()
                .is_some_and(|v| self.is_ref_returning_structured(&v)),
            _ => false,
        }
    }

    fn ref_returns_structured(&self, r: &ast::Ref) -> bool {
        r.name()
            .and_then(|name| self.ctx.type_ctx.def_id_for_name(self.ctx.interner, name.text()))
            .and_then(|def_id| self.ctx.type_ctx.def_type(def_id))
            .and_then(|def_type| self.ctx.type_ctx.type_shape(def_type))
            .is_some_and(|shape| {
                matches!(
                    shape,
                    TypeShape::Struct(_) | TypeShape::Enum(_) | TypeShape::Array { .. }
                )
            })
    }

    pub(super) fn collect_captures(pattern: &Pattern) -> HashSet<String> {
        fn collect(pattern: &Pattern, names: &mut HashSet<String>) {
            if let Pattern::CapturedPattern(cap) = pattern
                && let Some(name) = cap.name()
            {
                names.insert(name.text()[1..].to_string());
            }
            for child in pattern.children() {
                collect(&child, names);
            }
        }
        let mut names = HashSet::new();
        collect(pattern, &mut names);
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
pub fn needs_struct_wrapper(inner: &Pattern, type_ctx: &TypeContext) -> bool {
    let Some(info) = type_ctx.term_info(inner) else {
        return false;
    };

    // Must be a bubble (fields flow to parent scope)
    if !info.flow.has_fields() {
        return false;
    }

    info.flow
        .type_id()
        .and_then(|id| type_ctx.type_shape(id))
        .is_some_and(|shape| matches!(shape, TypeShape::Struct(_)))
}

/// Get row type ID for array element scoping.
pub fn row_type_id(inner: &Pattern, type_ctx: &TypeContext) -> Option<TypeId> {
    type_ctx
        .term_info(inner)
        .and_then(|info| info.flow.type_id())
}
