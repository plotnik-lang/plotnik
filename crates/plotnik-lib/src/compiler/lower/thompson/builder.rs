//! Core compiler state and entry points.

use std::collections::HashSet;

use indexmap::IndexMap;

use crate::bytecode::{Nav, SpanKind};
use crate::compiler::analyze::types::TypeShape;
use crate::compiler::analyze::types::type_shape::PatternFlow;
use crate::compiler::ids::DefId;
use crate::compiler::lower::LowerInput;
use crate::compiler::lower::ir::{
    CalleeEntry, EffectIR, InstructionIR, Label, NfaGraph, ReturnAddr, ReturnIR,
};
use crate::compiler::lower::spans::{SpanBindingIR, SpanId, SpanTable, assign_spans};
use crate::compiler::lower::verify::verify_fresh_build;
use crate::compiler::parse::ast::{self, Pattern};
use crate::compiler::parse::cst::SyntaxNode;

use super::capture::{CaptureEffects, PatternCtx};
use super::navigation::AnchorSemantics;
use super::scope::{CaptureExits, SkipExit, SplitExits, Struct};
use crate::compiler::analyze::nullability::compute_nullable_defs;
use crate::compiler::analyze::types::type_check::consumable_value_root;

/// NfaBuilder state for Thompson construction.
pub struct NfaBuilder<'a> {
    pub(super) ctx: &'a LowerInput<'a>,
    pub(super) anchor_semantics: AnchorSemantics<'a>,
    pub(super) instructions: Vec<InstructionIR>,
    pub(crate) next_label_id: u32,
    pub(super) def_entries: IndexMap<DefId, Label>,
    pub(super) def_entries_consuming: IndexMap<DefId, Label>,
    /// Stack of active struct scopes for capture lookup.
    /// Innermost scope is at the end.
    pub(super) scope_stack: Vec<Struct>,
    /// Non-zero while compiling under a suppressive capture (`@_`). The whole
    /// region compiles structurally: captures are inert, alternations emit no
    /// variant tags or null defaults. Only definition calls still produce
    /// output — shared code emits unconditionally — and the call site brackets
    /// them with SuppressBegin/SuppressEnd (`RefLowering::SuppressedCall`).
    pub(super) suppress_depth: u32,
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
            def_entries: IndexMap::new(),
            def_entries_consuming: IndexMap::new(),
            scope_stack: Vec::new(),
            suppress_depth: 0,
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

        for (def_id, _) in ctx.analysis.type_analysis.iter_def_output() {
            let label = compiler.fresh_label();
            compiler.def_entries.insert(def_id, label);
        }

        for (def_id, _) in ctx.analysis.type_analysis.iter_def_output() {
            compiler.compile_def(def_id);
        }

        let mut entrypoint_wrappers = IndexMap::new();
        for (def_id, _) in ctx.analysis.type_analysis.iter_def_output() {
            let wrapper = compiler.emit_entrypoint_wrapper(def_id);
            entrypoint_wrappers.insert(def_id, wrapper);
        }

        verify_fresh_build(&compiler.instructions);

        NfaGraph {
            instructions: compiler.instructions,
            def_entries: compiler.def_entries,
            def_entries_consuming: compiler.def_entries_consuming,
            entrypoint_wrappers,
            spans: compiler.spans,
        }
    }

    fn emit_entrypoint_wrapper(&mut self, def_id: DefId) -> Label {
        let return_label = self.fresh_label();
        self.instructions.push(ReturnIR::new(return_label).into());

        let output = self.ctx.analysis.type_analysis.expect_def_output(def_id);
        let output_shape = self.ctx.analysis.type_analysis.expect_type_shape(output);
        let wraps_struct = matches!(output_shape, TypeShape::Struct(_));

        let after_body = if wraps_struct {
            self.emit_struct_close_step(return_label)
        } else if matches!(output_shape, TypeShape::Node) {
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
            CalleeEntry(self.def_entries[&def_id]),
        );

        if wraps_struct {
            self.emit_struct_step(call)
        } else {
            call
        }
    }

    /// Generate a fresh label.
    pub(super) fn fresh_label(&mut self) -> Label {
        let l = Label(self.next_label_id);
        self.next_label_id += 1;
        l
    }

    fn compile_def(&mut self, def_id: DefId) {
        let name_sym = self.ctx.analysis.dependency_analysis.def_name_sym(def_id);
        let name = self.ctx.analysis.interner.resolve(name_sym);

        let body = self
            .ctx
            .symbol_table
            .body(name)
            .expect("analyzed definition has a body");

        let entry_label = self.def_entries[&def_id];

        // Return when stack is empty means Accept; when non-empty, pops frame to caller.
        let return_label = self.fresh_label();
        self.instructions.push(ReturnIR::new(return_label).into());

        // Definition bodies use StayExact navigation: match at current position only.
        // The caller (alternation, sequence, quantifier, or VM top-level) owns the search.
        // This ensures named definition calls don't advance past positions that other
        // alternation branches should try.
        let body_nav = Some(Nav::StayExact);

        // Definitions are compiled in normalized form: body -> Return
        // No Struct/EndStruct wrapper - that's the caller's responsibility (call-site scoping).
        // We still use with_scope for member index lookup during compilation.
        // The inline-stack entry keeps a nullable self-reference inside this
        // body (`A = (x (A) (y))?`) from inlining itself endlessly.
        let type_id = self.ctx.analysis.type_analysis.expect_def_output(def_id);
        let (body_exit, def_span) = self.bracket_def_body_exit(body, return_label);

        self.inline_stack.push(def_id);
        let body_entry = self.with_scope(type_id, |this| {
            let ctx = if consumable_value_root(body) {
                PatternCtx::with_value(body_exit, body_nav)
            } else {
                PatternCtx::with_nav(body_exit, body_nav)
            };
            this.dispatch_pattern(body, ctx)
        });
        self.inline_stack.pop();

        let body_entry = self.wrap_def_body_entry(body_entry, def_span);

        if body_entry != entry_label {
            self.emit_epsilon(entry_label, vec![body_entry]);
        }
    }

    pub(super) fn compile_consuming_def(&mut self, def_id: DefId) -> Label {
        if let Some(&entry) = self.def_entries_consuming.get(&def_id) {
            return entry;
        }

        let entry_label = self.fresh_label();
        self.def_entries_consuming.insert(def_id, entry_label);

        let name_sym = self.ctx.analysis.dependency_analysis.def_name_sym(def_id);
        let name = self.ctx.analysis.interner.resolve(name_sym);
        let body = self
            .ctx
            .symbol_table
            .body(name)
            .expect("analyzed definition has a body");

        let return_label = self.fresh_label();
        self.instructions.push(ReturnIR::new(return_label).into());

        let body_nav = Some(Nav::StayExact);
        let type_id = self.ctx.analysis.type_analysis.expect_def_output(def_id);
        let (body_match_exit, def_span) = self.bracket_def_body_exit(body, return_label);

        self.inline_stack.push(def_id);
        let body_entry = self.with_scope(type_id, |this| {
            this.compile_skippable_with_exits(
                body,
                SplitExits {
                    match_exit: body_match_exit,
                    skip_exit: SkipExit::Fail,
                },
                body_nav,
                CaptureEffects::default(),
                consumable_value_root(body),
            )
        });
        self.inline_stack.pop();

        let body_entry = self.wrap_def_body_entry(body_entry, def_span);

        if body_entry != entry_label {
            self.emit_epsilon(entry_label, vec![body_entry]);
        }

        entry_label
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
    /// - Alternations: effects go on each branch
    /// - Other wrappers: effects propagate through
    pub(super) fn dispatch_pattern(&mut self, pattern: &Pattern, ctx: PatternCtx) -> Label {
        let ctx = self.bracket_pattern_ctx(pattern, ctx);
        match pattern {
            Pattern::NodePattern(n) => self.compile_node_pattern(n, ctx),
            Pattern::TokenPattern(n) => self.compile_token_pattern(n, ctx),
            Pattern::SeqPattern(s) => self.compile_seq(s, ctx),
            Pattern::Union(u) => self.compile_union(u, ctx),
            Pattern::Enum(e) => {
                // Inference decides tagging by consumption: a consumed enum
                // flows `Value(enum)`; an unconsumed one degraded to a union
                // (fields or void) and compiles without variant scopes. A
                // suppressed region discards the value, so even a consumed
                // enum compiles structurally there.
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
                    self.compile_enum(e, ctx)
                } else {
                    self.compile_degraded_enum(e, ctx)
                }
            }
            Pattern::CapturedPattern(c) => {
                let PatternCtx {
                    exit,
                    nav,
                    capture,
                    value: _,
                } = ctx;
                self.compile_captured(c, c.inner(), nav, capture, CaptureExits::Single(exit))
            }
            Pattern::QuantifiedPattern(q) => self.compile_quantified(q, ctx),
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
            Pattern::Union(_) => (SpanKind::Union, false),
            Pattern::Enum(_) => (SpanKind::Enum, false),
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
