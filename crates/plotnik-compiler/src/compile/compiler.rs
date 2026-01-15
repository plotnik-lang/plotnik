//! Core compiler state and entry points.

use std::cell::RefCell;

use indexmap::IndexMap;
use plotnik_core::{Interner, NodeFieldId, NodeTypeId, Symbol};

use crate::analyze::symbol_table::SymbolTable;
use crate::analyze::type_check::{DefId, TypeContext};
use crate::bytecode::{InstructionIR, Label, ReturnIR, TrampolineIR};
use crate::emit::StringTableBuilder;
use crate::parser::Expr;
use plotnik_bytecode::Nav;

use super::capture::CaptureEffects;
use super::dce::remove_unreachable;
use super::epsilon_elim::eliminate_epsilons;
use super::lower::lower;
use super::error::{CompileError, CompileResult};
use super::scope::StructScope;
use super::verify::debug_verify_ir_fingerprint;

/// Compilation context bundling all shared compilation state.
///
/// Uses `RefCell` for `strings` to allow interior mutability while
/// sharing the context across compilation phases.
pub struct CompileCtx<'a> {
    pub interner: &'a Interner,
    pub type_ctx: &'a TypeContext,
    pub symbol_table: &'a SymbolTable,
    pub strings: &'a RefCell<StringTableBuilder>,
    pub node_types: Option<&'a IndexMap<Symbol, NodeTypeId>>,
    pub node_fields: Option<&'a IndexMap<Symbol, NodeFieldId>>,
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
    /// Create a new compiler with the given context.
    pub fn new(ctx: &'a CompileCtx<'a>) -> Self {
        Self {
            ctx,
            instructions: Vec::new(),
            next_label_id: 0,
            def_entries: IndexMap::new(),
            scope_stack: Vec::new(),
        }
    }

    /// Compile all definitions in the query.
    pub fn compile(ctx: &'a CompileCtx<'a>) -> Result<CompileResult, CompileError> {
        let mut compiler = Compiler::new(ctx);

        // Emit universal preamble first: Obj -> Trampoline -> EndObj -> Return
        // This wraps any entrypoint to create the top-level scope.
        let preamble_entry = compiler.emit_preamble();

        // Pre-allocate entry labels for all definitions
        for (def_id, _) in ctx.type_ctx.iter_def_types() {
            let label = compiler.fresh_label();
            compiler.def_entries.insert(def_id, label);
        }

        // Compile each definition
        for (def_id, _) in ctx.type_ctx.iter_def_types() {
            compiler.compile_def(def_id)?;
        }

        let mut result = CompileResult {
            instructions: compiler.instructions,
            def_entries: compiler.def_entries,
            preamble_entry,
        };

        // Eliminate epsilon transitions (with semantic verification in debug builds)
        eliminate_epsilons(&mut result, ctx);

        // Remove unreachable instructions (bypassed epsilons, etc.)
        remove_unreachable(&mut result);

        // Lower to bytecode-compatible form (cascade overflows)
        lower(&mut result);

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

        // Chain: Obj -> Trampoline -> EndObj -> Return
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

    /// Compile a single definition.
    fn compile_def(&mut self, def_id: DefId) -> Result<(), CompileError> {
        let name_sym = self.ctx.type_ctx.def_name_sym(def_id);
        let name = self.ctx.interner.resolve(name_sym);

        let Some(body) = self.ctx.symbol_table.get(name) else {
            return Err(CompileError::DefinitionNotFound(name.to_string()));
        };

        let entry_label = self.def_entries[&def_id];

        // Create Return instruction at definition exit.
        // When stack is empty, Return means Accept (top-level match completed).
        // When stack is non-empty, Return pops frame and jumps to return address.
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

        // If body_entry differs from our pre-allocated entry, emit an epsilon jump
        if body_entry != entry_label {
            self.emit_epsilon(entry_label, vec![body_entry]);
        }

        // Debug-only: verify IR semantic fingerprint
        debug_verify_ir_fingerprint(
            &self.instructions,
            entry_label,
            &self.def_entries,
            name,
            self.ctx,
        );

        Ok(())
    }

    /// Compile an expression with an optional navigation override.
    pub(super) fn compile_expr_with_nav(
        &mut self,
        expr: &Expr,
        exit: Label,
        nav_override: Option<Nav>,
    ) -> Label {
        self.compile_expr_inner(expr, exit, nav_override, CaptureEffects::default())
    }

    /// Compile an expression with navigation override and capture effects.
    ///
    /// Capture effects are propagated to the innermost match instruction:
    /// - Named/Anonymous nodes: effects go on the match
    /// - Sequences: effects go on last item
    /// - Alternations: effects go on each branch
    /// - Other wrappers: effects propagate through
    pub(super) fn compile_expr_inner(
        &mut self,
        expr: &Expr,
        exit: Label,
        nav_override: Option<Nav>,
        capture: CaptureEffects,
    ) -> Label {
        match expr {
            // Leaf nodes: attach capture effects directly
            Expr::NamedNode(n) => self.compile_named_node_inner(n, exit, nav_override, capture),
            Expr::AnonymousNode(n) => {
                self.compile_anonymous_node_inner(n, exit, nav_override, capture)
            }
            // Sequences: pass capture to last item
            Expr::SeqExpr(s) => self.compile_seq_inner(s, exit, nav_override, capture),
            // Alternations: pass capture to each branch
            Expr::AltExpr(a) => self.compile_alt_inner(a, exit, nav_override, capture),
            // Wrappers: propagate capture through
            Expr::CapturedExpr(c) => self.compile_captured_inner(c, exit, nav_override, capture),
            Expr::QuantifiedExpr(q) => {
                self.compile_quantified_inner(q, exit, nav_override, capture)
            }
            Expr::FieldExpr(f) => self.compile_field_inner(f, exit, nav_override, capture),
            // Refs: special handling for Call
            Expr::Ref(r) => self.compile_ref_inner(r, exit, nav_override, None, capture),
        }
    }
}
