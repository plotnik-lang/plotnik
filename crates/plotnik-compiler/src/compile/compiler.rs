//! Core compiler state and entry points.

use std::cell::RefCell;

use indexmap::IndexMap;
use plotnik_core::{Interner, NodeFieldId, NodeType, NodeTypeId, Symbol};

use crate::analyze::symbol_table::SymbolTable;
use crate::analyze::type_check::{DefId, TypeContext};
use crate::bytecode::{InstructionIR, Label, ReturnIR, TrampolineIR};
use crate::emit::StringTableBuilder;
use crate::parser::Expr;
use plotnik_bytecode::Nav;

use super::capture::ExprCtx;
use super::collapse_up::collapse_up;
use super::dce::remove_unreachable;
use super::epsilon_elim::eliminate_epsilons;
use super::error::{CompileError, CompileResult};
use super::lower::lower;
use super::scope::{CaptureExits, StructScope};
use super::verify::{run_verified, verify_constructed};

/// Compilation context bundling all shared compilation state.
///
/// Uses `RefCell` for `strings` to allow interior mutability while
/// sharing the context across compilation phases.
pub struct CompileCtx<'a> {
    pub interner: &'a Interner,
    pub type_ctx: &'a TypeContext,
    pub symbol_table: &'a SymbolTable,
    pub strings: &'a RefCell<StringTableBuilder>,
    pub node_types: &'a IndexMap<NodeType<Symbol>, NodeTypeId>,
    pub node_fields: &'a IndexMap<Symbol, NodeFieldId>,
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

    pub fn compile(ctx: &'a CompileCtx<'a>) -> Result<CompileResult, CompileError> {
        let mut compiler = Compiler::new(ctx);

        // Emit universal preamble first: Obj -> Trampoline -> EndObj -> Return
        // This wraps any entrypoint to create the top-level scope.
        let preamble_entry = compiler.emit_preamble();

        for (def_id, _) in ctx.type_ctx.iter_def_types() {
            let label = compiler.fresh_label();
            compiler.def_entries.insert(def_id, label);
        }

        for (def_id, _) in ctx.type_ctx.iter_def_types() {
            compiler.compile_def(def_id)?;
        }

        let mut result = CompileResult {
            instructions: compiler.instructions,
            def_entries: compiler.def_entries,
            preamble_entry,
        };

        // Each pass is wrapped so debug builds assert it preserved the IR's
        // order-sensitive semantic fingerprint and structural invariants. The
        // wrapping compiles to a direct call in release builds.
        verify_constructed(&result, ctx);
        run_verified("eliminate_epsilons", &mut result, ctx, eliminate_epsilons);
        run_verified("remove_unreachable", &mut result, ctx, remove_unreachable);
        run_verified("collapse_up", &mut result, ctx, collapse_up);
        run_verified("lower", &mut result, ctx, lower);

        Ok(result)
    }

    /// Emit the universal preamble: Obj -> Trampoline -> EndObj -> Return
    ///
    /// The preamble creates a scope for the entrypoint's captures.
    /// The Trampoline instruction jumps to the actual entrypoint (set via VM context).
    fn emit_preamble(&mut self) -> Label {
        // Return (stack is empty after preamble, so this means Accept)
        let return_label = self.fresh_label();
        self.instructions.push(ReturnIR::new(return_label).into());

        let endobj_label = self.emit_endobj_step(return_label);

        let trampoline_label = self.fresh_label();
        self.instructions
            .push(TrampolineIR::new(trampoline_label, endobj_label).into());

        self.emit_obj_step(trampoline_label)
    }

    /// Generate a fresh label.
    pub(super) fn fresh_label(&mut self) -> Label {
        let l = Label(self.next_label_id);
        self.next_label_id += 1;
        l
    }

    fn compile_def(&mut self, def_id: DefId) -> Result<(), CompileError> {
        let name_sym = self.ctx.type_ctx.def_name_sym(def_id);
        let name = self.ctx.interner.resolve(name_sym);

        let Some(body) = self.ctx.symbol_table.get(name) else {
            return Err(CompileError::DefinitionNotFound(name.to_string()));
        };

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
        // No Obj/EndObj wrapper - that's the caller's responsibility (call-site scoping).
        // We still use with_scope for member index lookup during compilation.
        let body_entry = if let Some(type_id) = self.ctx.type_ctx.get_def_type(def_id) {
            self.with_scope(type_id, |this| {
                this.compile_expr_with_nav(body, return_label, body_nav)
            })
        } else {
            self.compile_expr_with_nav(body, return_label, body_nav)
        };

        if body_entry != entry_label {
            self.emit_epsilon(entry_label, vec![body_entry]);
        }

        Ok(())
    }

    pub(super) fn compile_expr_with_nav(
        &mut self,
        expr: &Expr,
        exit: Label,
        nav_override: Option<Nav>,
    ) -> Label {
        self.compile_expr_inner(expr, ExprCtx::with_nav(exit, nav_override))
    }

    /// Compile an expression with navigation override and capture effects.
    ///
    /// Capture effects are propagated to the innermost match instruction:
    /// - Named/Anonymous nodes: effects go on the match
    /// - Sequences: effects go on last item
    /// - Alternations: effects go on each branch
    /// - Other wrappers: effects propagate through
    pub(super) fn compile_expr_inner(&mut self, expr: &Expr, ctx: ExprCtx) -> Label {
        match expr {
            Expr::NamedNode(n) => self.compile_named_node_inner(n, ctx),
            Expr::AnonymousNode(n) => self.compile_anonymous_node_inner(n, ctx),
            Expr::SeqExpr(s) => self.compile_seq_inner(s, ctx),
            Expr::AltExpr(a) => self.compile_alt_inner(a, ctx),
            Expr::CapturedExpr(c) => {
                let ExprCtx { exit, nav, capture } = ctx;
                self.compile_captured(c, c.inner(), nav, capture, CaptureExits::Single(exit))
            }
            Expr::QuantifiedExpr(q) => self.compile_quantified_inner(q, ctx),
            Expr::FieldExpr(f) => self.compile_field_inner(f, ctx),
            Expr::Ref(r) => self.compile_ref_inner(r, ctx, None),
        }
    }
}
