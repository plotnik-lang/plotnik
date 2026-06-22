//! Core compiler state and entry points.

use indexmap::IndexMap;
use plotnik_compiler_core::{DependencyAnalysis, GrammarBinding};
use plotnik_core::Interner;

use crate::analyze::symbol_table::SymbolTable;
use crate::analyze::type_check::{DefId, TypeContext};
use crate::bytecode::{InstructionIR, Label, ReturnIR, TrampolineIR};
use crate::parser::Pattern;
use plotnik_bytecode::Nav;

use super::capture::ExprCtx;
use super::error::CompileResult;
use super::scope::{CaptureExits, StructScope};
use super::verify::verify_constructed;

/// Compilation context bundling all shared compilation state.
///
pub struct CompileCtx<'a> {
    pub interner: &'a Interner,
    pub type_ctx: &'a TypeContext,
    pub symbol_table: &'a SymbolTable,
    pub grammar: &'a GrammarBinding,
    pub dependency_analysis: &'a DependencyAnalysis,
}

/// Compiler state for Thompson construction.
pub struct Compiler<'a> {
    pub(super) ctx: &'a CompileCtx<'a>,
    pub(super) instructions: Vec<InstructionIR>,
    pub(crate) next_label_id: u32,
    pub(super) def_entries: IndexMap<DefId, Label>,
    /// Stack of active struct scopes for capture lookup.
    /// Innermost scope is at the end.
    pub(super) scope_stack: Vec<StructScope>,
}

impl<'a> Compiler<'a> {
    pub fn new(ctx: &'a CompileCtx<'a>) -> Self {
        Self {
            ctx,
            instructions: Vec::new(),
            next_label_id: 0,
            def_entries: IndexMap::new(),
            scope_stack: Vec::new(),
        }
    }

    pub fn build_ir(ctx: &'a CompileCtx<'a>) -> CompileResult {
        let mut compiler = Compiler::new(ctx);

        // Emit universal preamble first: Struct -> Trampoline -> EndStruct -> Return
        // This wraps any entrypoint to create the top-level scope.
        let preamble_entry = compiler.emit_preamble();

        for (def_id, _) in ctx.type_ctx.iter_def_types() {
            let label = compiler.fresh_label();
            compiler.def_entries.insert(def_id, label);
        }

        for (def_id, _) in ctx.type_ctx.iter_def_types() {
            compiler.compile_def(def_id);
        }

        let result = CompileResult {
            instructions: compiler.instructions,
            def_entries: compiler.def_entries,
            preamble_entry,
        };

        verify_constructed(&result, ctx);

        result
    }

    /// Emit the universal preamble: Struct -> Trampoline -> EndStruct -> Return
    ///
    /// The preamble creates a scope for the entrypoint's captures.
    /// The Trampoline instruction jumps to the actual entrypoint (set via VM context).
    fn emit_preamble(&mut self) -> Label {
        // Return (stack is empty after preamble, so this means Accept)
        let return_label = self.fresh_label();
        self.instructions.push(ReturnIR::new(return_label).into());

        let struct_close_label = self.emit_struct_close_step(return_label);

        let trampoline_label = self.fresh_label();
        self.instructions
            .push(TrampolineIR::new(trampoline_label, struct_close_label).into());

        self.emit_struct_step(trampoline_label)
    }

    /// Generate a fresh label.
    pub(super) fn fresh_label(&mut self) -> Label {
        let l = Label(self.next_label_id);
        self.next_label_id += 1;
        l
    }

    fn compile_def(&mut self, def_id: DefId) {
        let name_sym = self.ctx.type_ctx.def_name_sym(def_id);
        let name = self.ctx.interner.resolve(name_sym);

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
        let body_entry = if let Some(type_id) = self.ctx.type_ctx.def_type(def_id) {
            self.with_scope(type_id, |this| {
                this.compile_pattern(body, return_label, body_nav)
            })
        } else {
            self.compile_pattern(body, return_label, body_nav)
        };

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
        self.dispatch_pattern(pattern, ExprCtx::with_nav(exit, nav_override))
    }

    /// Compile an expression with navigation override and capture effects.
    ///
    /// Capture effects are propagated to the innermost match instruction:
    /// - Named/Anonymous nodes: effects go on the match
    /// - Sequences: effects go on last item
    /// - Alternations: effects go on each branch
    /// - Other wrappers: effects propagate through
    pub(super) fn dispatch_pattern(&mut self, pattern: &Pattern, ctx: ExprCtx) -> Label {
        match pattern {
            Pattern::NodePattern(n) => self.compile_node_pattern(n, ctx),
            Pattern::TokenPattern(n) => self.compile_token_pattern(n, ctx),
            Pattern::SeqPattern(s) => self.compile_seq(s, ctx),
            Pattern::Union(u) => self.compile_union(u, ctx),
            Pattern::Enum(e) => self.compile_enum(e, ctx),
            Pattern::CapturedPattern(c) => {
                let ExprCtx { exit, nav, capture } = ctx;
                self.compile_captured(c, c.inner(), nav, capture, CaptureExits::Single(exit))
            }
            Pattern::QuantifiedPattern(q) => self.compile_quantified(q, ctx),
            Pattern::FieldPattern(f) => self.compile_field(f, ctx),
            Pattern::Ref(r) => self.compile_ref(r, ctx, None),
        }
    }
}
