//! Core compiler state and entry points.

use std::collections::HashSet;

use indexmap::IndexMap;

use crate::bytecode::{Nav, SpanKind};
use crate::compiler::analyze::types::TypeShape;
use crate::compiler::analyze::types::type_shape::PatternFlow;
use crate::compiler::ids::DefId;
use crate::compiler::lower::LowerInput;
use crate::compiler::lower::ir::{
    CalleeEntry, DefBodyMode, DefOutputMode, DefSpecialization, EffectIR, InstructionIR, Label,
    LabelOrigin, NfaGraph, ReturnAddr, ReturnIR,
};
use crate::compiler::lower::spans::{SpanBindingIR, SpanId, SpanTable, assign_spans};
use crate::compiler::lower::verify::verify_fresh_build;
use crate::compiler::parse::ast::{self, Pattern};
use crate::compiler::parse::cst::SyntaxNode;

use super::capture::{CaptureEffects, PatternCtx};
use super::navigation::AnchorSemantics;
use super::scope::{CaptureExits, RecordScope, SkipExit};
use crate::compiler::analyze::nullability::compute_nullable_defs;
use crate::compiler::analyze::types::type_check::definition_value_root;

/// NfaBuilder state for Thompson construction.
pub struct NfaBuilder<'a> {
    pub(super) ctx: &'a LowerInput<'a>,
    pub(super) anchor_semantics: AnchorSemantics<'a>,
    pub(super) instructions: Vec<InstructionIR>,
    pub(crate) next_label_id: u32,
    /// Compilation window every fresh label is attributed to (see [`LabelOrigin`]).
    current_origin: Option<LabelOrigin>,
    /// Origin per allocated label id (index = `Label.0`), moved into the graph.
    label_origins: Vec<Option<LabelOrigin>>,
    pub(super) def_entries: IndexMap<DefSpecialization, Label>,
    compiled_def_specializations: HashSet<DefSpecialization>,
    active_def_specializations: HashSet<DefSpecialization>,
    /// Stack of active record scopes for capture lookup.
    /// Innermost scope is at the end.
    pub(super) scope_stack: Vec<RecordScope>,
    /// Non-zero while compiling under a discard (`@_`). The whole
    /// region compiles structurally: captures are inert, alternations emit no
    /// variant tags or null defaults. Only definition calls still produce
    /// output — shared code emits unconditionally — and the call site brackets
    /// them with SuppressBegin/SuppressEnd (`RefLowering::SuppressedCall`).
    pub(super) suppress_depth: u32,
    /// Non-zero while explicit node-pattern matches contribute scalar provenance.
    source_mark_depth: u32,
    /// Definitions whose body can match zero nodes; references to them are
    /// inlined at the call site (see `nullability`).
    pub(super) nullable_defs: HashSet<DefId>,
    /// Definitions whose body is currently being compiled (standalone or
    /// inlined). A nullable reference back into this set cannot inline again —
    /// it falls back to a guarded call (`compile_ref_guarded_call`).
    pub(super) inline_stack: Vec<DefId>,
    /// Inspection span table, built before lowering so construct ids are stable.
    pub(super) spans: Option<SpanTable>,
}

impl<'a> NfaBuilder<'a> {
    pub(in crate::compiler::lower) fn new(ctx: &'a LowerInput<'a>) -> Self {
        Self {
            ctx,
            anchor_semantics: AnchorSemantics::new(ctx.symbol_table),
            instructions: Vec::new(),
            next_label_id: 0,
            current_origin: None,
            label_origins: Vec::new(),
            def_entries: IndexMap::new(),
            compiled_def_specializations: HashSet::new(),
            active_def_specializations: HashSet::new(),
            scope_stack: Vec::new(),
            suppress_depth: 0,
            source_mark_depth: 0,
            nullable_defs: compute_nullable_defs(
                ctx.analysis.interner,
                ctx.symbol_table,
                ctx.analysis.dependency_analysis,
            ),
            inline_stack: Vec::new(),
            spans: None,
        }
    }

    pub(super) fn is_suppressed(&self) -> bool {
        self.suppress_depth > 0
    }

    pub(super) fn with_suppression<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.suppress_depth += 1;
        let result = f(self);
        self.suppress_depth -= 1;
        result
    }

    pub(super) fn with_source_marking<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.source_mark_depth += 1;
        let result = f(self);
        self.source_mark_depth -= 1;
        result
    }

    pub(super) fn marks_source(&self) -> bool {
        self.source_mark_depth > 0
    }

    pub(super) fn records_inspection(&self) -> bool {
        self.ctx.inspection
    }

    /// The pre-assigned span id for a construct, or `None` when inspection is off
    /// or that construct's tier was dropped by the budget ladder.
    pub(super) fn span_id(&self, node: &SyntaxNode, kind: SpanKind) -> Option<SpanId> {
        self.spans
            .as_ref()
            .and_then(|spans| spans.lookup(node, kind))
    }

    pub(super) fn bind_span(&mut self, id: SpanId, binding: SpanBindingIR) {
        let spans = self
            .spans
            .as_mut()
            .expect("span binding requires inspection span table");
        spans.bind(id, binding);
    }

    pub(in crate::compiler::lower) fn build_ir(ctx: &'a LowerInput<'a>) -> NfaGraph {
        let mut compiler = NfaBuilder::new(ctx);
        compiler.spans = ctx.inspection.then(|| assign_spans(ctx).table);

        for (def_id, _) in ctx.analysis.type_analysis.iter_entry_point_outputs() {
            compiler.ensure_def_specialization(DefSpecialization::ordinary(def_id));
        }

        let mut entry_point_wrappers = IndexMap::new();
        for (def_id, _) in ctx.analysis.type_analysis.iter_entry_point_outputs() {
            compiler.current_origin = Some(LabelOrigin::Wrapper(def_id));
            let wrapper = compiler.emit_entry_point_wrapper(def_id);
            entry_point_wrappers.insert(def_id, wrapper);
        }

        verify_fresh_build(&compiler.instructions);
        debug_assert_eq!(
            compiler.label_origins.len(),
            compiler.next_label_id as usize,
            "every label must be minted through fresh_label, or origins desync"
        );

        NfaGraph {
            instructions: compiler.instructions,
            def_entries: compiler.def_entries,
            entry_point_wrappers,
            spans: compiler.spans,
            label_origins: compiler.label_origins,
        }
    }

    fn emit_entry_point_wrapper(&mut self, def_id: DefId) -> Label {
        let return_label = self.fresh_label();
        self.instructions.push(ReturnIR::new(return_label).into());

        let output = self.ctx.analysis.type_analysis.expect_def_output(def_id);
        let output_shape = output
            .value()
            .map(|type_id| self.ctx.analysis.type_analysis.expect_type_shape(type_id));
        let wraps_record = matches!(output_shape, Some(TypeShape::Record(_)));

        let after_body = if wraps_record {
            self.emit_record_close(return_label)
        } else if matches!(output_shape, Some(TypeShape::Node)) {
            self.emit_effects_epsilon(
                return_label,
                vec![EffectIR::node()],
                CaptureEffects::default(),
            )
        } else {
            return_label
        };
        let call = self.emit_call(
            Nav::Stay,
            None,
            ReturnAddr(after_body),
            CalleeEntry(self.def_entries[&DefSpecialization::ordinary(def_id)]),
        );

        if wraps_record {
            self.emit_record_open(call)
        } else {
            call
        }
    }

    /// Generate a fresh label, attributing it to the active compilation window.
    pub(super) fn fresh_label(&mut self) -> Label {
        let l = Label(self.next_label_id);
        self.next_label_id += 1;
        self.label_origins.push(self.current_origin);
        l
    }

    /// Return the entry for one semantic definition specialization, compiling it
    /// once when first requested. The entry is registered before the body so a
    /// recursive component can call back into an active specialization safely.
    pub(super) fn ensure_def_specialization(&mut self, specialization: DefSpecialization) -> Label {
        let entry = self.reserve_def_specialization(&specialization);
        if self.compiled_def_specializations.contains(&specialization)
            || !self
                .active_def_specializations
                .insert(specialization.clone())
        {
            return entry;
        }

        let previous_origin = self
            .current_origin
            .replace(Self::specialization_origin(&specialization));
        self.compile_def_specialization_body(&specialization, entry);
        self.current_origin = previous_origin;

        let removed = self.active_def_specializations.remove(&specialization);
        assert!(removed, "compiled definition specialization was active");
        self.compiled_def_specializations.insert(specialization);
        entry
    }

    fn reserve_def_specialization(&mut self, specialization: &DefSpecialization) -> Label {
        if let Some(&entry) = self.def_entries.get(specialization) {
            return entry;
        }

        let previous_origin = self
            .current_origin
            .replace(Self::specialization_origin(specialization));
        let entry = self.fresh_label();
        self.current_origin = previous_origin;
        self.def_entries.insert(specialization.clone(), entry);
        entry
    }

    fn specialization_origin(specialization: &DefSpecialization) -> LabelOrigin {
        if specialization.is_ordinary() {
            return LabelOrigin::Def(specialization.def_id());
        }

        LabelOrigin::DefSpecialization {
            def_id: specialization.def_id(),
            output: specialization.mode().output().origin(),
            source: specialization.mode().source(),
            route: specialization.route(),
        }
    }

    fn compile_def_specialization_body(
        &mut self,
        specialization: &DefSpecialization,
        entry_label: Label,
    ) {
        let def_id = specialization.def_id();
        let name_sym = self.ctx.analysis.dependency_analysis.def_name_sym(def_id);
        let name = self.ctx.analysis.interner.resolve(name_sym);

        let body = self
            .ctx
            .symbol_table
            .body(name)
            .expect("analyzed definition has a body");

        let matched_return = self.fresh_label();
        let matched_return_instr = match specialization.route() {
            crate::compiler::lower::ir::DefRoute::Caller => ReturnIR::matched(matched_return),
            crate::compiler::lower::ir::DefRoute::Routed { .. } => {
                ReturnIR::routed_matched(matched_return)
            }
        };
        self.instructions.push(matched_return_instr.into());
        let exits = if specialization.route().splits() {
            let empty_return = self.fresh_label();
            self.instructions
                .push(ReturnIR::routed_empty(empty_return).into());
            CaptureExits::Split {
                match_exit: matched_return,
                skip_exit: SkipExit::To(empty_return),
            }
        } else if specialization.route().requires_consumption() {
            CaptureExits::Split {
                match_exit: matched_return,
                skip_exit: SkipExit::Fail,
            }
        } else {
            CaptureExits::Single(matched_return)
        };

        // Ordinary specializations are exact because their caller owns navigation.
        // Routed recursive specializations own the original call-site navigation so
        // their authored nullable alternative order stays above candidate retries.
        let body_nav = Some(specialization.route().body_nav());

        // Definitions are compiled in normalized form: body -> Return
        // No record wrapper - that's the caller's responsibility (call-site scoping).
        // We still use with_scope for member index lookup during compilation.
        // The inline-stack entry keeps a nullable self-reference inside this
        // body (`A = (x (A) (y))?`) from inlining itself endlessly.
        let type_id = self
            .ctx
            .analysis
            .type_analysis
            .expect_def_output(def_id)
            .value();
        let (body_exits, def_span) = self.bracket_def_body_exits(body, exits);

        self.inline_stack.push(def_id);
        let mode = specialization.mode().clone();
        let body_entry = self.with_scope_if_present(type_id, |this| {
            this.compile_def_body(body, &mode, body_exits, body_nav)
        });
        self.inline_stack.pop();

        let body_entry = self.wrap_def_body_entry(body_entry, def_span);

        if body_entry != entry_label {
            self.emit_epsilon(entry_label, vec![body_entry]);
        }
    }

    fn compile_def_body(
        &mut self,
        body: &Pattern,
        mode: &DefBodyMode,
        exits: CaptureExits,
        nav: Option<Nav>,
    ) -> Label {
        if mode.marks_source() {
            return self
                .with_source_marking(|this| this.compile_def_output(body, mode, exits, nav));
        }
        self.compile_def_output(body, mode, exits, nav)
    }

    fn compile_def_output(
        &mut self,
        body: &Pattern,
        mode: &DefBodyMode,
        exits: CaptureExits,
        nav: Option<Nav>,
    ) -> Label {
        if let DefOutputMode::CaptureType(plan) = mode.output() {
            return self.capture_type(plan, nav, exits).definition(body);
        }

        if mode.suppresses_output() {
            return self
                .with_suppression(|this| this.compile_structural_def_body(body, exits, nav));
        }

        self.compile_structural_def_body(body, exits, nav)
    }

    fn compile_structural_def_body(
        &mut self,
        body: &Pattern,
        exits: CaptureExits,
        nav: Option<Nav>,
    ) -> Label {
        if let CaptureExits::Split {
            match_exit,
            skip_exit,
        } = exits
        {
            let pattern_ctx = PatternCtx {
                exit: match_exit,
                nav,
                capture: CaptureEffects::default(),
                value: definition_value_root(body),
            };
            return self.compile_nullable_pattern(body, pattern_ctx, skip_exit);
        }

        let CaptureExits::Single(exit) = exits else {
            unreachable!("split definition exits returned above")
        };

        let ctx = if definition_value_root(body) {
            PatternCtx::with_value(exit, nav)
        } else {
            PatternCtx::with_nav(exit, nav)
        };
        self.dispatch_pattern(body, ctx)
    }

    fn bracket_def_body_exits(
        &mut self,
        body: &Pattern,
        exits: CaptureExits,
    ) -> (CaptureExits, Option<SpanId>) {
        match exits {
            CaptureExits::Single(exit) => {
                let (exit, span) = self.bracket_def_body_exit(body, exit);
                (CaptureExits::Single(exit), span)
            }
            CaptureExits::Split {
                match_exit,
                skip_exit,
            } => {
                let (match_exit, span) = self.bracket_def_body_exit(body, match_exit);
                let skip_exit = match skip_exit {
                    SkipExit::To(exit) => SkipExit::To(self.bracket_def_body_exit(body, exit).0),
                    SkipExit::Fail => SkipExit::Fail,
                };
                (
                    CaptureExits::Split {
                        match_exit,
                        skip_exit,
                    },
                    span,
                )
            }
        }
    }

    pub(super) fn compile_pattern(
        &mut self,
        pattern: &Pattern,
        exit: Label,
        nav_override: Option<Nav>,
    ) -> Label {
        self.dispatch_pattern(pattern, PatternCtx::with_nav(exit, nav_override))
    }

    /// Compile a pattern with navigation override and capture effects.
    ///
    /// Capture effects are propagated to the innermost match instruction:
    /// - Named/Anonymous nodes: effects go on the match
    /// - Sequences: effects go on last item
    /// - Alternations: effects go on each alternative
    /// - Other wrappers: effects propagate through
    pub(super) fn dispatch_pattern(&mut self, pattern: &Pattern, ctx: PatternCtx) -> Label {
        let ctx = self.bracket_pattern_ctx(pattern, ctx);
        match pattern {
            Pattern::NodePattern(n) => self.compile_node_pattern(n, ctx),
            Pattern::TokenPattern(n) => self.compile_token_pattern(n, ctx),
            Pattern::SeqPattern(s) => self.compile_seq(s, ctx),
            Pattern::Alternation(alternation) => {
                // Inference decides tagging from output context. A labeled
                // alternation has `Value(variant)` flow where its value is
                // materialized; in a fields context, its captures merge. A
                // discarded region compiles structurally without variant scopes.
                let flow = &self
                    .ctx
                    .analysis
                    .type_analysis
                    .expect_pattern_result(pattern)
                    .flow;
                if ctx.consumes_value()
                    && matches!(flow, PatternFlow::Value(_))
                    && !self.is_suppressed()
                {
                    self.compile_labeled_alternation(alternation, ctx)
                } else {
                    self.compile_unlabeled_alternation(alternation, ctx)
                }
            }
            Pattern::CapturedPattern(c) => {
                let PatternCtx {
                    exit,
                    nav,
                    capture,
                    value: _,
                } = ctx;
                self.compile_captured(c, nav, capture, CaptureExits::Single(exit))
            }
            Pattern::QuantifiedPattern(q) => self.compile_quantified_pattern(q, ctx),
            Pattern::FieldPattern(f) => self.compile_field(f, ctx),
            Pattern::DefRef(r) => self.compile_ref(r, ctx, None),
        }
    }

    /// Wrap this pattern's capture channel in inspection span brackets.
    ///
    /// Node and token patterns use `SpanStartAt` because their `pre` effects land
    /// on the consuming match instruction; epsilon-entered constructs use pure
    /// marker starts.
    pub(super) fn bracket_pattern_ctx(&mut self, pattern: &Pattern, ctx: PatternCtx) -> PatternCtx {
        let (kind, start_at) = match pattern {
            Pattern::NodePattern(_) | Pattern::TokenPattern(_) => (SpanKind::Pattern, true),
            Pattern::SeqPattern(_) => (SpanKind::Sequence, false),
            Pattern::Alternation(alternation) => (
                match alternation.labeling() {
                    ast::Labeling::Labeled => SpanKind::LabeledAlternation,
                    ast::Labeling::Unlabeled | ast::Labeling::Mixed => {
                        SpanKind::UnlabeledAlternation
                    }
                },
                false,
            ),
            Pattern::DefRef(_) => (SpanKind::Ref, false),
            _ => return ctx,
        };
        let Some(id) = self.span_id(pattern.syntax(), kind) else {
            return ctx;
        };

        let start = if start_at {
            EffectIR::span_start_at(id.0)
        } else {
            EffectIR::span_start(id.0)
        };
        let PatternCtx {
            exit,
            nav,
            capture,
            value,
        } = ctx;
        PatternCtx {
            exit,
            nav,
            capture: capture.nest_span(start, EffectIR::span_end(id.0)),
            value,
        }
    }

    pub(super) fn bracket_def_body_exit(
        &mut self,
        body: &Pattern,
        exit: Label,
    ) -> (Label, Option<SpanId>) {
        let Some(id) = self.def_body_span_id(body) else {
            return (exit, None);
        };

        let close = self.emit_effects_epsilon(
            exit,
            vec![EffectIR::span_end(id.0)],
            CaptureEffects::default(),
        );
        (close, Some(id))
    }

    pub(super) fn wrap_def_body_entry(&mut self, entry: Label, span_id: Option<SpanId>) -> Label {
        let Some(id) = span_id else {
            return entry;
        };

        self.wrap_entry_pre(entry, vec![EffectIR::span_start(id.0)])
    }

    fn def_body_span_id(&self, body: &Pattern) -> Option<SpanId> {
        let def = body
            .syntax()
            .parent()
            .and_then(ast::Def::cast)
            .expect("definition body must have a Def parent");
        self.span_id(def.syntax(), SpanKind::Def)
    }
}
