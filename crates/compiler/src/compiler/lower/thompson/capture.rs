use crate::bytecode::{EffectKind, Nav, SpanKind};
use crate::compiler::analyze::result::CaptureScopeKind;
use crate::compiler::analyze::types::{CaptureKind, TypeAnalysis, TypeShape};
use crate::compiler::ids::TypeId;
use crate::compiler::lower::ir::{EffectIR, Label};
use crate::compiler::lower::spans::SpanBindingIR;
use crate::compiler::parse::ast::{self, Pattern};

use super::NfaBuilder;
use super::scope::RecordScope;

#[derive(Clone, Default)]
pub struct CaptureEffects {
    pub pre: Vec<EffectIR>,
    pub post: Vec<EffectIR>,
}

impl CaptureEffects {
    pub fn new(pre: Vec<EffectIR>, post: Vec<EffectIR>) -> Self {
        Self { pre, post }
    }

    pub fn new_post(post: Vec<EffectIR>) -> Self {
        Self { pre: vec![], post }
    }

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
        assert!(
            matches!(
                (open.kind(), close.kind()),
                (EffectKind::RecordOpen, EffectKind::RecordClose)
                    | (EffectKind::VariantOpen, EffectKind::VariantClose)
                    | (EffectKind::ListOpen, EffectKind::ListClose)
                    | (EffectKind::SuppressBegin, EffectKind::SuppressEnd)
            ),
            "capture lowering received mismatched scope effects: opening={:?}, closing={:?}",
            open.kind(),
            close.kind()
        );
        self.pre.push(open);
        self.post.insert(0, close);
        self
    }

    pub fn with_pre_values(mut self, effects: Vec<EffectIR>) -> Self {
        self.pre.extend(effects);
        self
    }

    pub fn with_post_values(mut self, effects: Vec<EffectIR>) -> Self {
        self.post.splice(0..0, effects);
        self
    }

    pub fn post_attaches_value(&self) -> bool {
        self.pre
            .iter()
            .rev()
            .find(|effect| !effect.is_span_marker())
            .is_some_and(|effect| effect.kind() == EffectKind::RecordSet)
            || Self::effects_attach_value(&self.post)
    }

    pub fn effects_attach_value(effects: &[EffectIR]) -> bool {
        effects
            .iter()
            .find(|e| !e.is_span_marker())
            .is_some_and(|e| matches!(e.kind(), EffectKind::RecordSet | EffectKind::ArrayPush))
    }

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
            | EffectKind::TextClose
            | EffectKind::BoolClose => return Some(i),
            _ => {}
        }
    }
    None
}

#[derive(Clone)]
pub(super) struct PatternCtx {
    pub exit: Label,
    pub nav: Option<Nav>,
    pub capture: CaptureEffects,
    pub observe_value: bool,
}

impl PatternCtx {
    pub(super) fn with_nav(exit: Label, nav: Option<Nav>) -> Self {
        Self {
            exit,
            nav,
            capture: CaptureEffects::default(),
            observe_value: false,
        }
    }

    pub(super) fn observing_value(exit: Label, nav: Option<Nav>) -> Self {
        Self {
            exit,
            nav,
            capture: CaptureEffects::default(),
            observe_value: true,
        }
    }

    pub(super) fn needs_value(&self) -> bool {
        self.observe_value || self.capture.post_attaches_value()
    }
}

impl NfaBuilder<'_> {
    pub(super) fn build_capture_effects(
        &mut self,
        captured_pattern: &ast::CapturedPattern,
        mechanism: CaptureKind,
    ) -> Vec<EffectIR> {
        let capture = captured_pattern.capture();
        let mut effects = Vec::with_capacity(4);
        let mut member_ref = None;

        if let Some(name_token) = capture.name() {
            let capture_name = &name_token.text()[1..];
            if let Some(RecordScope(type_id)) = self.scope_stack.last().copied()
                && self
                    .ctx
                    .result
                    .layout()
                    .scope(type_id)
                    .is_some_and(|scope| scope.kind() == CaptureScopeKind::Record)
            {
                let member = self
                    .lookup_member(capture_name, type_id)
                    .expect("captured field must resolve in the current scope");
                effects.push(EffectIR::with_member(EffectKind::RecordSet, member));
                member_ref = Some(member);
            }
        }

        if mechanism == CaptureKind::Node {
            effects.push(EffectIR::node());
        }

        if let Some(id) = self.span_id(capture.syntax(), SpanKind::Capture) {
            if let Some(member_ref) = member_ref {
                self.bind_span(id, SpanBindingIR::Member(member_ref));
            }
            effects.insert(0, EffectIR::span_start(id.0));
            effects.push(EffectIR::span_end(id.0));
        }

        effects
    }

    pub(super) fn quantifier_needs_node_for_push(&self, pattern: &Pattern) -> bool {
        let Pattern::QuantifiedPattern(quant) = pattern else {
            return true;
        };
        let Some(inner) = quant.inner() else {
            return true;
        };

        self.element_needs_node(&inner)
    }

    pub(super) fn element_needs_node(&self, element: &Pattern) -> bool {
        if self.is_ref_returning_structured(element) {
            return false;
        }

        let flow = self.ctx.analysis.type_analysis.expect_pattern_flow(element);

        !flow
            .type_id()
            .map(|id| self.ctx.analysis.type_analysis.expect_type_shape(id))
            .is_some_and(|shape| matches!(shape, TypeShape::Record(_) | TypeShape::Variant(_)))
    }

    pub(super) fn is_ref_returning_structured(&self, pattern: &Pattern) -> bool {
        match pattern {
            Pattern::DefRef(_) => self
                .ctx
                .analysis
                .type_analysis
                .ref_returns_structured(pattern, self.ctx.analysis.definitions),
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

pub fn needs_record_wrapper(inner: &Pattern, type_ctx: &TypeAnalysis) -> bool {
    let flow = type_ctx.expect_pattern_flow(inner);

    if !flow.has_fields() {
        return false;
    }

    flow.type_id()
        .map(|id| type_ctx.expect_type_shape(id))
        .is_some_and(|shape| matches!(shape, TypeShape::Record(_)))
}

pub fn element_type_id(inner: &Pattern, type_ctx: &TypeAnalysis) -> Option<TypeId> {
    type_ctx.expect_pattern_flow(inner).type_id()
}
