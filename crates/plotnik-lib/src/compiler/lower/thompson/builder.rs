//! Core compiler state and entry points.

use std::collections::HashSet;

use indexmap::IndexMap;

use crate::bytecode::Nav;
use crate::compiler::analyze::types::TypeShape;
use crate::compiler::analyze::types::type_shape::PatternFlow;
use crate::compiler::ids::DefId;
use crate::compiler::lower::LowerInput;
use crate::compiler::lower::ir::{
    CalleeEntry, InstructionIR, Label, NfaGraph, ReturnAddr, ReturnIR,
};
use crate::compiler::parse::ast::Pattern;

use super::capture::PatternCtx;
use super::navigation::AnchorSemantics;
use super::scope::{CaptureExits, Struct};
use crate::compiler::analyze::nullability::compute_nullable_defs;

/// NfaBuilder state for Thompson construction.
pub struct NfaBuilder<'a> {
    pub(super) ctx: &'a LowerInput<'a>,
    pub(super) anchor_semantics: AnchorSemantics<'a>,
    pub(super) instructions: Vec<InstructionIR>,
    pub(crate) next_label_id: u32,
    pub(super) def_entries: IndexMap<DefId, Label>,
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
}

impl<'a> NfaBuilder<'a> {
    pub(in crate::compiler::lower) fn new(ctx: &'a LowerInput<'a>) -> Self {
        Self {
            ctx,
            anchor_semantics: AnchorSemantics::new(ctx.symbol_table),
            instructions: Vec::new(),
            next_label_id: 0,
            def_entries: IndexMap::new(),
            scope_stack: Vec::new(),
            suppress_depth: 0,
            nullable_defs: compute_nullable_defs(
                ctx.analysis.interner,
                ctx.symbol_table,
                ctx.analysis.dependency_analysis,
            ),
            inline_stack: Vec::new(),
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

    pub(in crate::compiler::lower) fn build_ir(ctx: &'a LowerInput<'a>) -> NfaGraph {
        let mut compiler = NfaBuilder::new(ctx);

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

        NfaGraph {
            instructions: compiler.instructions,
            def_entries: compiler.def_entries,
            entrypoint_wrappers,
        }
    }

    fn emit_entrypoint_wrapper(&mut self, def_id: DefId) -> Label {
        let return_label = self.fresh_label();
        self.instructions.push(ReturnIR::new(return_label).into());

        let output = self.ctx.analysis.type_analysis.expect_def_output(def_id);
        let wraps_struct = matches!(
            self.ctx.analysis.type_analysis.expect_type_shape(output),
            TypeShape::Struct(_)
        );

        let after_body = if wraps_struct {
            self.emit_struct_close_step(return_label)
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
        self.inline_stack.push(def_id);
        let body_entry = self.with_scope(type_id, |this| {
            this.compile_pattern(body, return_label, body_nav)
        });
        self.inline_stack.pop();

        if body_entry != entry_label {
            self.emit_epsilon(entry_label, vec![body_entry]);
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
    /// - Alternations: effects go on each branch
    /// - Other wrappers: effects propagate through
    pub(super) fn dispatch_pattern(&mut self, pattern: &Pattern, ctx: PatternCtx) -> Label {
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
                if matches!(flow, PatternFlow::Value(_)) && !self.is_suppressed() {
                    self.compile_enum(e, ctx)
                } else {
                    self.compile_degraded_enum(e, ctx)
                }
            }
            Pattern::CapturedPattern(c) => {
                let PatternCtx { exit, nav, capture } = ctx;
                self.compile_captured(c, c.inner(), nav, capture, CaptureExits::Single(exit))
            }
            Pattern::QuantifiedPattern(q) => self.compile_quantified(q, ctx),
            Pattern::FieldPattern(f) => self.compile_field(f, ctx),
            Pattern::DefRef(r) => self.compile_ref(r, ctx, None),
        }
    }
}
