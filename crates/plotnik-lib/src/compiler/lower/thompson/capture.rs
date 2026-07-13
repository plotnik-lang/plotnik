//! Capture effects handling for query compilation.
//!
//! Manages the construction and propagation of capture effects (`Node` + `RecordSet`).
//! through the compilation pipeline.

use crate::bytecode::{EffectKind, Nav, SpanKind};
use crate::compiler::analyze::types::{CaptureKind, TypeAnalysis, TypeShape};
use crate::compiler::ids::TypeId;
use crate::compiler::lower::ir::{EffectIR, Label};
use crate::compiler::lower::spans::SpanBindingIR;
use crate::compiler::parse::ast::{self, Pattern};

use super::NfaBuilder;
use super::scope::RecordScope;

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
/// For labeled alternations `[A: body]`:
/// - `pre` contains `VariantOpen(case)` for alternative entry
/// - `post` contains `VariantClose` for alternative exit
#[derive(Clone, Default)]
pub struct CaptureEffects {
    /// Effects to place before the compiled subgraph's own effects.
    pub pre: Vec<EffectIR>,
    /// Effects to place after the compiled subgraph's own effects.
    pub post: Vec<EffectIR>,
}

impl CaptureEffects {
    pub fn new(pre: Vec<EffectIR>, post: Vec<EffectIR>) -> Self {
        Self { pre, post }
    }

    pub fn new_post(post: Vec<EffectIR>) -> Self {
        Self { pre: vec![], post }
    }

    /// Add an inner scope (opens after existing scopes, closes before them).
    ///
    /// Use for paired record, variant, list, and suppression effects.
    ///
    /// Given existing `pre=[A_Open]`, `post=[A_Close]`, adding inner scope B:
    /// - Result: `pre=[A_Open, B_Open]`, `post=[B_Close, A_Close]`
    /// - Execution: A opens -> B opens -> match -> B closes -> A closes
    pub fn nest_scope(mut self, open: EffectIR, close: EffectIR) -> Self {
        assert!(
            matches!(
                open.kind(),
                EffectKind::RecordOpen
                    | EffectKind::VariantOpen
                    | EffectKind::ListOpen
                    | EffectKind::SuppressBegin
            ),
            "nest_scope expects scope-opening effect, got {:?}",
            open.kind()
        );
        assert!(
            matches!(
                close.kind(),
                EffectKind::RecordClose
                    | EffectKind::VariantClose
                    | EffectKind::ListClose
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
    /// Use for default-value injection in unlabeled alternations.
    ///
    /// Given `pre=[Scope_Open]`, adding value effects:
    /// - Result: `pre=[Scope_Open, Value1, Value2]`
    pub fn with_pre_values(mut self, effects: Vec<EffectIR>) -> Self {
        self.pre.extend(effects);
        self
    }

    /// Add post-match value effects (run before any scope closes).
    ///
    /// Use for `Node` + `RecordSet` capture effects and `ArrayPush` for lists.
    ///
    /// Given `post=[Scope_Close]`, adding value effects:
    /// - Result: `post=[Value1, Value2, Scope_Close]`
    pub fn with_post_values(mut self, effects: Vec<EffectIR>) -> Self {
        self.post.splice(0..0, effects);
        self
    }

    /// Whether the first trailing effect consumes a value the inner pattern
    /// leaves pending. Producer effects like `Node` are not consumers; they
    /// capture the matched node themselves.
    pub fn post_consumes_value(&self) -> bool {
        Self::effects_consume_value(&self.post)
    }

    pub fn effects_consume_value(effects: &[EffectIR]) -> bool {
        effects
            .iter()
            .find(|e| !e.is_span_marker())
            .is_some_and(|e| matches!(e.kind(), EffectKind::RecordSet | EffectKind::ArrayPush))
    }

    /// Wrap this channel in a construct's span brackets. The start runs after
    /// enclosing opens, and the end runs before the first close that belongs to
    /// an enclosing scope.
    pub(super) fn nest_span(mut self, start: EffectIR, end: EffectIR) -> Self {
        self.pre.push(start);
        let pos = first_unmatched_close(&self.post).unwrap_or(self.post.len());
        self.post.insert(pos, end);
        self
    }
}

pub(super) fn first_unmatched_close(post: &[EffectIR]) -> Option<usize> {
    let mut span_depth: u32 = 0;
    for (i, effect) in post.iter().enumerate() {
        match effect.kind() {
            EffectKind::SpanStart | EffectKind::SpanStartAt => span_depth += 1,
            EffectKind::SpanEnd => {
                if span_depth == 0 {
                    return Some(i);
                }
                span_depth -= 1;
            }
            EffectKind::ListClose
            | EffectKind::RecordClose
            | EffectKind::VariantClose
            | EffectKind::SuppressEnd
            | EffectKind::StrClose
            | EffectKind::BoolClose => return Some(i),
            _ => {}
        }
    }
    None
}

/// The backbone calling convention threaded through the `dispatch_pattern` family.
///
/// Bundles the values every pattern compiler needs: where the compiled
/// fragment continues (`exit`), the navigation it should apply to reach its first
/// candidate (`nav`, `None` meaning "use the form's default"), and the capture
/// effects that land on its innermost match/scope-close instruction (`capture`).
/// `value` marks contexts where the pattern's own pending value is observed,
/// such as a definition-root quantifier or a set-after structured capture.
#[derive(Clone)]
pub(super) struct PatternCtx {
    pub exit: Label,
    pub nav: Option<Nav>,
    pub capture: CaptureEffects,
    pub value: bool,
}

impl PatternCtx {
    pub(super) fn with_nav(exit: Label, nav: Option<Nav>) -> Self {
        Self {
            exit,
            nav,
            capture: CaptureEffects::default(),
            value: false,
        }
    }

    pub(super) fn with_value(exit: Label, nav: Option<Nav>) -> Self {
        Self {
            exit,
            nav,
            capture: CaptureEffects::default(),
            value: true,
        }
    }

    pub(super) fn consumes_value(&self) -> bool {
        self.value || self.capture.post_consumes_value()
    }
}

impl NfaBuilder<'_> {
    /// Build capture effects (`Node` + `RecordSet`) for a capture whose inner was
    /// classified as `mechanism` (or `None` for a bare `@x`).
    ///
    /// The caller already classifies the inner to dispatch, so it passes the
    /// mechanism in rather than have this re-classify the same inner.
    pub(super) fn build_capture_effects(
        &mut self,
        cap: &ast::CapturedPattern,
        mechanism: Option<CaptureKind>,
    ) -> Vec<EffectIR> {
        let mut effects = Vec::with_capacity(4);
        let mut member_ref = None;

        // Only the `Node` mechanism captures the matched node directly. Every
        // other mechanism (record scope, pass-through ref/variant/forward, list)
        // produces its value via RecordClose/VariantClose/ListClose/Call, so the capture itself
        // emits no Node. A bare capture (`@x` with no inner) is a Node.
        let is_node_mechanism = mechanism.is_none_or(|m| m == CaptureKind::Node);
        if is_node_mechanism {
            effects.push(EffectIR::node());
        }

        // Add `RecordSet` if we have a capture name.
        // Always look up in the current scope - bubble captures don't create new scopes,
        // so all fields (including nested bubble captures) reference the same root struct.
        if let Some(name_token) = cap.name() {
            let capture_name = &name_token.text()[1..];
            // Suppressed regions never reach here (their captures are inert), so
            // the enclosing scope is a struct at every real capture site — except
            // a variant-rooted definition body, whose scope carries no fields. Once
            // a struct scope exists, a missing member is our bug.
            if let Some(RecordScope(type_id)) = self.scope_stack.last().copied()
                && self
                    .ctx
                    .analysis
                    .type_analysis
                    .record_fields(type_id)
                    .is_some()
            {
                let member = self
                    .lookup_member(capture_name, type_id)
                    .expect("captured field must resolve in the current scope");
                effects.push(EffectIR::with_member(EffectKind::RecordSet, member));
                member_ref = Some(member);
            }
        }

        if let Some(id) = self.span_id(cap.syntax(), SpanKind::Capture) {
            if let Some(member_ref) = member_ref {
                self.bind_span(id, SpanBindingIR::Member(member_ref));
            }
            effects.insert(0, EffectIR::span_start(id.0));
            effects.push(EffectIR::span_end(id.0));
        }

        effects
    }

    /// Check if a quantifier body needs `Node` before `ArrayPush`.
    ///
    /// For node list elements, we need `[Node, ArrayPush]`.
    /// to capture the matched node value.
    /// For structured elements, RecordClose/VariantClose provides the value.
    /// For refs returning structured types, Call provides the value.
    pub(super) fn quantifier_needs_node_for_push(&self, pattern: &Pattern) -> bool {
        let Pattern::QuantifiedPattern(quant) = pattern else {
            return true;
        };
        let Some(inner) = quant.inner() else {
            return true;
        };

        self.element_needs_node(&inner)
    }

    /// Whether a quantifier element needs a `Node` effect to produce its value.
    ///
    /// A ref returning a structured type leaves its value pending via Call/Return;
    /// a struct- or variant-shaped element leaves it pending via RecordClose/VariantClose.
    /// Everything else (a plain node match) needs an explicit `Node`.
    pub(super) fn element_needs_node(&self, element: &Pattern) -> bool {
        if self.is_ref_returning_structured(element) {
            return false;
        }

        // Check the actual inferred type, not syntax. Compile runs after the type
        // analysis is frozen, so every pattern it visits has a recorded result.
        let info = self
            .ctx
            .analysis
            .type_analysis
            .expect_pattern_result(element);

        !info
            .flow
            .type_id()
            .map(|id| self.ctx.analysis.type_analysis.expect_type_shape(id))
            .is_some_and(|shape| matches!(shape, TypeShape::Record(_) | TypeShape::Variant(_)))
    }

    /// Check if pattern is (or wraps) a ref returning a structured type.
    ///
    /// For such refs, we skip the Node effect in captures - the Call leaves
    /// the structured result pending for `RecordSet` to consume.
    pub(super) fn is_ref_returning_structured(&self, pattern: &Pattern) -> bool {
        match pattern {
            Pattern::DefRef(_) => self.ctx.analysis.type_analysis.ref_returns_structured(
                pattern,
                self.ctx.analysis.dependency_analysis,
                self.ctx.analysis.interner,
            ),
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
}

/// Check if inner needs a record wrapper for list iterations.
///
/// Returns true when the inner pattern produces a record type (bubbling fields).
/// This includes:
/// - Sequences/alternations with captures: `{(a) @x (b) @y}*`
/// - Named nodes with bubble captures: `(node (child) @x)*`
///
/// Variant types use VariantOpen/VariantClose instead (handled separately).
pub fn needs_record_wrapper(inner: &Pattern, type_ctx: &TypeAnalysis) -> bool {
    let info = type_ctx.expect_pattern_result(inner);

    // Must be a bubble (fields flow to parent scope)
    if !info.flow.has_fields() {
        return false;
    }

    info.flow
        .type_id()
        .map(|id| type_ctx.expect_type_shape(id))
        .is_some_and(|shape| matches!(shape, TypeShape::Record(_)))
}

/// Get the element type ID for list-element scoping.
pub fn element_type_id(inner: &Pattern, type_ctx: &TypeAnalysis) -> Option<TypeId> {
    type_ctx.expect_pattern_result(inner).flow.type_id()
}
