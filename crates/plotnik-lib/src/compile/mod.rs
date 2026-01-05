//! Thompson-like NFA construction for query compilation.
//!
//! Compiles query AST expressions into bytecode IR with symbolic labels.
//! Labels are resolved to concrete StepIds during the layout phase.
//! Member indices use deferred resolution via MemberRef for correct absolute indices.
//!
//! # Module Organization
//!
//! The compiler is split into focused modules:
//! - `capture`: Capture effects handling (Node/Text + Set)
//! - `expressions`: Leaf expression compilation (named/anon nodes, refs, fields, captures)
//! - `navigation`: Navigation mode computation for anchors and quantifiers
//! - `quantifier`: Unified quantifier compilation (*, +, ?)
//! - `scope`: Scope management for struct/array wrappers
//! - `sequences`: Sequence and alternation compilation

mod capture;
mod expressions;
mod navigation;
mod quantifier;
mod scope;
mod sequences;

use indexmap::IndexMap;
use plotnik_core::{Interner, NodeFieldId, NodeTypeId, Symbol};

use crate::bytecode::ir::{EffectIR, Instruction, Label, MatchIR, ReturnIR};
use crate::bytecode::{EffectOpcode, Nav};
use crate::parser::ast::Expr;

use crate::emit::StringTableBuilder;
use crate::analyze::symbol_table::SymbolTable;
use crate::analyze::type_check::{DefId, TypeContext, TypeShape};

pub use capture::CaptureEffects;
use scope::StructScope;

/// Error during compilation.
#[derive(Clone, Debug)]
pub enum CompileError {
    /// Definition not found in symbol table.
    DefinitionNotFound(String),
    /// Expression body missing.
    MissingBody(String),
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DefinitionNotFound(name) => write!(f, "definition not found: {name}"),
            Self::MissingBody(name) => write!(f, "missing body for definition: {name}"),
        }
    }
}

impl std::error::Error for CompileError {}

/// Result of compilation.
#[derive(Clone, Debug)]
pub struct CompileResult {
    /// All generated instructions.
    pub instructions: Vec<Instruction>,
    /// Entry labels for each definition (in definition order).
    pub def_entries: IndexMap<DefId, Label>,
}

/// Compiler state for Thompson construction.
pub struct Compiler<'a> {
    pub(super) interner: &'a Interner,
    pub(super) type_ctx: &'a TypeContext,
    symbol_table: &'a SymbolTable,
    pub(super) strings: &'a mut StringTableBuilder,
    pub(super) node_type_ids: Option<&'a IndexMap<Symbol, NodeTypeId>>,
    pub(super) node_field_ids: Option<&'a IndexMap<Symbol, NodeFieldId>>,
    pub(super) instructions: Vec<Instruction>,
    next_label_id: u32,
    pub(super) def_entries: IndexMap<DefId, Label>,
    /// Stack of active struct scopes for capture lookup.
    /// Innermost scope is at the end.
    pub(super) scope_stack: Vec<StructScope>,
}

impl<'a> Compiler<'a> {
    /// Create a new compiler.
    pub fn new(
        interner: &'a Interner,
        type_ctx: &'a TypeContext,
        symbol_table: &'a SymbolTable,
        strings: &'a mut StringTableBuilder,
        node_type_ids: Option<&'a IndexMap<Symbol, NodeTypeId>>,
        node_field_ids: Option<&'a IndexMap<Symbol, NodeFieldId>>,
    ) -> Self {
        Self {
            interner,
            type_ctx,
            symbol_table,
            strings,
            node_type_ids,
            node_field_ids,
            instructions: Vec::new(),
            next_label_id: 0,
            def_entries: IndexMap::new(),
            scope_stack: Vec::new(),
        }
    }

    /// Compile all definitions in the query.
    pub fn compile(
        interner: &'a Interner,
        type_ctx: &'a TypeContext,
        symbol_table: &'a SymbolTable,
        strings: &'a mut StringTableBuilder,
        node_type_ids: Option<&'a IndexMap<Symbol, NodeTypeId>>,
        node_field_ids: Option<&'a IndexMap<Symbol, NodeFieldId>>,
    ) -> Result<CompileResult, CompileError> {
        let mut compiler = Self::new(interner, type_ctx, symbol_table, strings, node_type_ids, node_field_ids);

        // Pre-allocate entry labels for all definitions
        for (def_id, _) in type_ctx.iter_def_types() {
            let label = compiler.fresh_label();
            compiler.def_entries.insert(def_id, label);
        }

        // Compile each definition
        for (def_id, _) in type_ctx.iter_def_types() {
            compiler.compile_def(def_id)?;
        }

        Ok(CompileResult {
            instructions: compiler.instructions,
            def_entries: compiler.def_entries,
        })
    }

    /// Generate a fresh label.
    pub(super) fn fresh_label(&mut self) -> Label {
        let l = Label(self.next_label_id);
        self.next_label_id += 1;
        l
    }

    /// Compile a single definition.
    fn compile_def(&mut self, def_id: DefId) -> Result<(), CompileError> {
        let name_sym = self.type_ctx.def_name_sym(def_id);
        let name = self.interner.resolve(name_sym);

        let Some(body) = self.symbol_table.get(name) else {
            return Err(CompileError::DefinitionNotFound(name.to_string()));
        };

        let entry_label = self.def_entries[&def_id];

        // Create Return instruction at definition exit.
        // When stack is empty, Return means Accept (top-level match completed).
        // When stack is non-empty, Return pops frame and jumps to return address.
        let return_label = self.fresh_label();
        self.instructions
            .push(Instruction::Return(ReturnIR { label: return_label }));

        // Check if definition needs Obj/EndObj wrapper.
        // A definition needs its own scope when:
        // 1. It returns a struct type, AND
        // 2. It has direct captures (CapturedExpr not inside a Ref)
        //
        // When captures come only from Refs (called definitions), those definitions
        // already handle their own Obj/EndObj scopes. Adding another wrapper would
        // create nested scopes where the inner result gets lost.
        let def_returns_struct = self
            .type_ctx
            .get_def_type(def_id)
            .and_then(|tid| self.type_ctx.get_type(tid))
            .is_some_and(|shape| matches!(shape, TypeShape::Struct(_)));
        let has_direct_captures = !Self::collect_captures(body).is_empty();
        let needs_obj_wrapper = def_returns_struct && has_direct_captures;

        // Definition bodies use StayExact navigation: match at current position only.
        // The caller (alternation, sequence, quantifier, or VM top-level) owns the search.
        // This ensures named definition calls don't advance past positions that other
        // alternation branches should try.
        let body_nav = Some(Nav::StayExact);

        let body_entry = if needs_obj_wrapper {
            let type_id = self.type_ctx.get_def_type(def_id).expect("checked above");

            // Emit EndObj → Return
            let endobj_label = self.fresh_label();
            self.instructions.push(Instruction::Match(MatchIR {
                label: endobj_label,
                nav: Nav::Stay,
                node_type: None,
                node_field: None,
                pre_effects: vec![],
                neg_fields: vec![],
                post_effects: vec![EffectIR::simple(EffectOpcode::EndObj, 0)],
                successors: vec![return_label],
            }));

            // Compile body with scope, targeting EndObj
            let inner_entry = self.with_scope(type_id, |this| {
                this.compile_expr_with_nav(body, endobj_label, body_nav)
            });

            // Emit Obj → inner_entry
            let obj_label = self.fresh_label();
            self.instructions.push(Instruction::Match(MatchIR {
                label: obj_label,
                nav: Nav::Stay,
                node_type: None,
                node_field: None,
                pre_effects: vec![EffectIR::simple(EffectOpcode::Obj, 0)],
                neg_fields: vec![],
                post_effects: vec![],
                successors: vec![inner_entry],
            }));

            obj_label
        } else if let Some(type_id) = self.type_ctx.get_def_type(def_id) {
            self.with_scope(type_id, |this| this.compile_expr_with_nav(body, return_label, body_nav))
        } else {
            self.compile_expr_with_nav(body, return_label, body_nav)
        };

        // If body_entry differs from our pre-allocated entry, emit an epsilon jump
        if body_entry != entry_label {
            self.emit_epsilon(entry_label, vec![body_entry]);
        }

        Ok(())
    }

    /// Compile an expression with an optional navigation override.
    pub(super) fn compile_expr_with_nav(&mut self, expr: &Expr, exit: Label, nav_override: Option<Nav>) -> Label {
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
            Expr::AnonymousNode(n) => self.compile_anonymous_node_inner(n, exit, nav_override, capture),
            // Sequences: pass capture to last item
            Expr::SeqExpr(s) => self.compile_seq_inner(s, exit, nav_override, capture),
            // Alternations: pass capture to each branch
            Expr::AltExpr(a) => self.compile_alt_inner(a, exit, nav_override, capture),
            // Wrappers: propagate capture through
            Expr::CapturedExpr(c) => self.compile_captured_inner(c, exit, nav_override, capture),
            Expr::QuantifiedExpr(q) => self.compile_quantified_inner(q, exit, nav_override, capture),
            Expr::FieldExpr(f) => self.compile_field_inner(f, exit, nav_override, capture),
            // Refs: special handling for Call
            Expr::Ref(r) => self.compile_ref_inner(r, exit, nav_override, None, capture),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::QueryBuilder;

    #[test]
    fn compile_simple_named_node() {
        let query = QueryBuilder::one_liner("Test = (identifier)")
            .parse()
            .unwrap()
            .analyze();

        let mut strings = StringTableBuilder::new();
        let result = Compiler::compile(
            query.interner(),
            query.type_context(),
            &query.symbol_table,
            &mut strings,
            None,
            None,
        )
        .unwrap();

        // Should have at least one instruction
        assert!(!result.instructions.is_empty());
        // Should have one entrypoint
        assert_eq!(result.def_entries.len(), 1);
    }

    #[test]
    fn compile_alternation() {
        let query = QueryBuilder::one_liner("Test = [(identifier) (number)]")
            .parse()
            .unwrap()
            .analyze();

        let mut strings = StringTableBuilder::new();
        let result = Compiler::compile(
            query.interner(),
            query.type_context(),
            &query.symbol_table,
            &mut strings,
            None,
            None,
        )
        .unwrap();

        assert!(!result.instructions.is_empty());
    }

    #[test]
    fn compile_sequence() {
        let query = QueryBuilder::one_liner("Test = {(comment) (function)}")
            .parse()
            .unwrap()
            .analyze();

        let mut strings = StringTableBuilder::new();
        let result = Compiler::compile(
            query.interner(),
            query.type_context(),
            &query.symbol_table,
            &mut strings,
            None,
            None,
        )
        .unwrap();

        assert!(!result.instructions.is_empty());
    }

    #[test]
    fn compile_quantified() {
        let query = QueryBuilder::one_liner("Test = (identifier)*")
            .parse()
            .unwrap()
            .analyze();

        let mut strings = StringTableBuilder::new();
        let result = Compiler::compile(
            query.interner(),
            query.type_context(),
            &query.symbol_table,
            &mut strings,
            None,
            None,
        )
        .unwrap();

        assert!(!result.instructions.is_empty());
    }

    #[test]
    fn compile_capture() {
        let query = QueryBuilder::one_liner("Test = (identifier) @id")
            .parse()
            .unwrap()
            .analyze();

        let mut strings = StringTableBuilder::new();
        let result = Compiler::compile(
            query.interner(),
            query.type_context(),
            &query.symbol_table,
            &mut strings,
            None,
            None,
        )
        .unwrap();

        assert!(!result.instructions.is_empty());
    }

    #[test]
    fn compile_nested() {
        let query = QueryBuilder::one_liner("Test = (call_expression function: (identifier) @fn)")
            .parse()
            .unwrap()
            .analyze();

        let mut strings = StringTableBuilder::new();
        let result = Compiler::compile(
            query.interner(),
            query.type_context(),
            &query.symbol_table,
            &mut strings,
            None,
            None,
        )
        .unwrap();

        assert!(!result.instructions.is_empty());
    }
}
