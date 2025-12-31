//! Thompson-like NFA construction for query compilation.
//!
//! Compiles query AST expressions into bytecode IR with symbolic labels.
//! Labels are resolved to concrete StepIds during the layout phase.

use std::num::NonZeroU16;

use indexmap::IndexMap;
use plotnik_core::{Interner, NodeFieldId, NodeTypeId, Symbol};

use crate::bytecode::ir::{CallIR, Instruction, Label, MatchIR, ReturnIR};
use crate::bytecode::{EffectOp, EffectOpcode, Nav};
use crate::parser::ast::{self, Expr, SeqItem};
use crate::parser::cst::SyntaxKind;

use super::codegen::StringTableBuilder;
use super::symbol_table::SymbolTable;
use super::type_check::{Arity, DefId, TypeContext, TypeShape};

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
    interner: &'a Interner,
    type_ctx: &'a TypeContext,
    symbol_table: &'a SymbolTable,
    strings: &'a mut StringTableBuilder,
    node_type_ids: Option<&'a IndexMap<Symbol, NodeTypeId>>,
    node_field_ids: Option<&'a IndexMap<Symbol, NodeFieldId>>,
    instructions: Vec<Instruction>,
    next_label_id: u32,
    def_entries: IndexMap<DefId, Label>,
    ref_id_counter: u16,
    /// Currently compiling definition (for member index lookup).
    current_def_id: Option<DefId>,
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
            ref_id_counter: 0,
            current_def_id: None,
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
    fn fresh_label(&mut self) -> Label {
        let l = Label(self.next_label_id);
        self.next_label_id += 1;
        l
    }

    /// Generate a fresh ref_id for Call/Return verification.
    fn fresh_ref_id(&mut self) -> u16 {
        let id = self.ref_id_counter;
        self.ref_id_counter += 1;
        id
    }

    /// Compile a single definition.
    fn compile_def(&mut self, def_id: DefId) -> Result<(), CompileError> {
        let name_sym = self.type_ctx.def_name_sym(def_id);
        let name = self.interner.resolve(name_sym);

        let Some(body) = self.symbol_table.get(name) else {
            return Err(CompileError::DefinitionNotFound(name.to_string()));
        };

        let entry_label = self.def_entries[&def_id];

        // Track current definition for member index lookup
        self.current_def_id = Some(def_id);

        // Compile body, targeting accept state
        let body_entry = self.compile_expr(body, Label::ACCEPT);

        // If body_entry differs from our pre-allocated entry, emit an epsilon jump
        if body_entry != entry_label {
            self.emit_epsilon(entry_label, vec![body_entry]);
        }

        self.current_def_id = None;
        Ok(())
    }

    /// Compile an expression, returning its entry label.
    fn compile_expr(&mut self, expr: &Expr, exit: Label) -> Label {
        match expr {
            Expr::NamedNode(n) => self.compile_named_node(n, exit),
            Expr::AnonymousNode(n) => self.compile_anonymous_node(n, exit),
            Expr::Ref(r) => self.compile_ref(r, exit),
            Expr::SeqExpr(s) => self.compile_seq(s, exit),
            Expr::AltExpr(a) => self.compile_alt(a, exit),
            Expr::QuantifiedExpr(q) => self.compile_quantified(q, exit),
            Expr::FieldExpr(f) => self.compile_field(f, exit),
            Expr::CapturedExpr(c) => self.compile_captured(c, exit),
        }
    }

    /// Compile a named node: `(identifier)` or `(call_expression arg: ...)`.
    fn compile_named_node(&mut self, node: &ast::NamedNode, exit: Label) -> Label {
        self.compile_named_node_with_nav(node, exit, None)
    }

    /// Check if an expression is anonymous (string literal or wildcard).
    fn expr_is_anonymous(&self, expr: Option<&Expr>) -> bool {
        matches!(expr, Some(Expr::AnonymousNode(_)))
    }

    /// Check for trailing anchor in items, looking inside sequences if needed.
    /// Returns (has_trailing_anchor, is_strict).
    fn check_trailing_anchor(&self, items: &[SeqItem]) -> (bool, bool) {
        // Direct trailing anchor
        if matches!(items.last(), Some(SeqItem::Anchor(_))) {
            let prev_expr = items.iter().rev().skip(1).find_map(|item| {
                if let SeqItem::Expr(e) = item { Some(e) } else { None }
            });
            return (true, self.expr_is_anonymous(prev_expr));
        }

        // Check if only child is a sequence with trailing anchor
        if items.len() == 1
            && let Some(SeqItem::Expr(Expr::SeqExpr(seq))) = items.first()
        {
            let seq_items: Vec<_> = seq.items().collect();
            return self.check_trailing_anchor(&seq_items);
        }

        (false, false)
    }

    /// Compile sequence items (expressions and anchors).
    ///
    /// Handles anchor semantics:
    /// - First child uses Down, subsequent use Next
    /// - Anchors determine strictness (Skip vs Exact)
    /// - Leading anchors affect first child navigation
    /// - Trailing anchors affect Up instruction (handled by caller)
    fn compile_seq_items(&mut self, items: &[SeqItem], exit: Label, is_inside_node: bool) -> Label {
        self.compile_seq_items_with_first_nav(items, exit, is_inside_node, None)
    }

    /// Compile sequence items with optional external nav override for first item.
    fn compile_seq_items_with_first_nav(
        &mut self,
        items: &[SeqItem],
        exit: Label,
        is_inside_node: bool,
        first_nav: Option<Nav>,
    ) -> Label {
        // Compute navigation modes first (immutable borrow)
        let mut nav_modes = Self::compute_nav_modes(items, is_inside_node);

        if nav_modes.is_empty() {
            return exit;
        }

        // Apply navigation to first expression:
        // 1. External nav (first_nav) takes precedence if provided
        // 2. If no external nav but inside node, default to Down
        // 3. Internal anchors (already computed) override these defaults
        if let Some((_, first_mode)) = nav_modes.first_mut()
            && first_mode.is_none()
        {
            *first_mode = first_nav.or_else(|| is_inside_node.then_some(Nav::Down));
        }

        // Build chain in reverse: last expression exits to `exit`, each prior exits to next
        let mut current_exit = exit;
        for (expr_idx, nav_override) in nav_modes.into_iter().rev() {
            let expr = items[expr_idx].as_expr().expect("nav_modes only contains expr indices");
            current_exit = self.compile_expr_with_nav(expr, current_exit, nav_override);
        }
        current_exit
    }

    /// Compute navigation modes for each expression based on anchor context.
    /// Returns a vector of (expression index, nav mode) pairs.
    fn compute_nav_modes(items: &[SeqItem], is_inside_node: bool) -> Vec<(usize, Option<Nav>)> {
        let mut result = Vec::new();
        let mut pending_anchor = false;
        let mut prev_is_anonymous = false;
        let mut is_first_expr = true;

        for (idx, item) in items.iter().enumerate() {
            match item {
                SeqItem::Anchor(_) => {
                    pending_anchor = true;
                }
                SeqItem::Expr(expr) => {
                    let current_is_anonymous = matches!(expr, Expr::AnonymousNode(_));
                    let nav = if pending_anchor {
                        // Anchor between previous item and this one
                        let is_exact = prev_is_anonymous || current_is_anonymous;
                        if is_first_expr && is_inside_node {
                            // First child with leading anchor
                            Some(if is_exact { Nav::DownExact } else { Nav::DownSkip })
                        } else if !is_first_expr {
                            // Sibling with anchor
                            Some(if is_exact { Nav::NextExact } else { Nav::NextSkip })
                        } else {
                            // First in sequence (not inside node)
                            None
                        }
                    } else if !is_first_expr {
                        // Normal sibling navigation (no anchor)
                        Some(Nav::Next)
                    } else {
                        // First expression - use default (None for sequences, Down for nodes)
                        None
                    };

                    result.push((idx, nav));
                    pending_anchor = false;
                    prev_is_anonymous = current_is_anonymous;
                    is_first_expr = false;
                }
            }
        }

        result
    }

    /// Compile an expression with an optional navigation override.
    fn compile_expr_with_nav(&mut self, expr: &Expr, exit: Label, nav_override: Option<Nav>) -> Label {
        match expr {
            // For expressions that emit their own Match instructions,
            // we need to modify the navigation mode
            Expr::NamedNode(n) => self.compile_named_node_with_nav(n, exit, nav_override),
            Expr::AnonymousNode(n) => self.compile_anonymous_node_with_nav(n, exit, nav_override),
            // For sequences and alternations, propagate nav to first item(s)
            Expr::SeqExpr(s) => self.compile_seq_with_nav(s, exit, nav_override),
            Expr::AltExpr(a) => self.compile_alt_with_nav(a, exit, nav_override),
            // For wrapper expressions, propagate nav to inner expression
            Expr::CapturedExpr(c) => self.compile_captured_with_nav(c, exit, nav_override),
            Expr::QuantifiedExpr(q) => self.compile_quantified_with_nav(q, exit, nav_override),
            Expr::FieldExpr(f) => self.compile_field_with_nav(f, exit, nav_override),
            Expr::Ref(r) => self.compile_ref_with_nav(r, exit, nav_override),
        }
    }

    /// Compile a named node: `(identifier)` or `(call_expression arg: ...)`.
    ///
    /// If `nav_override` is provided, uses that navigation instead of the default `Down`.
    fn compile_named_node_with_nav(&mut self, node: &ast::NamedNode, exit: Label, nav_override: Option<Nav>) -> Label {
        let entry = self.fresh_label();
        let node_type = self.resolve_node_type(node);
        let nav = nav_override.unwrap_or(Nav::Down);

        // Collect items and negated fields
        let items: Vec<_> = node.items().collect();
        let neg_fields = self.collect_neg_fields(node);

        // If no items, just match and exit
        if items.is_empty() {
            self.instructions.push(Instruction::Match(MatchIR {
                label: entry,
                nav,
                node_type,
                node_field: None,
                pre_effects: vec![],
                neg_fields,
                post_effects: vec![],
                successors: vec![exit],
            }));
            return entry;
        }

        // Determine Up navigation based on trailing anchor
        let (has_trailing_anchor, trailing_strictness) = self.check_trailing_anchor(&items);

        // With items: nav → items → Up → exit
        let up_label = self.fresh_label();
        let items_entry = self.compile_seq_items(&items, up_label, true);

        // Emit Up instruction with appropriate strictness
        let up_nav = if has_trailing_anchor {
            if trailing_strictness {
                Nav::UpExact(1)
            } else {
                Nav::UpSkipTrivia(1)
            }
        } else {
            Nav::Up(1)
        };
        self.instructions.push(Instruction::Match(MatchIR {
            label: up_label,
            nav: up_nav,
            node_type: None,
            node_field: None,
            pre_effects: vec![],
            neg_fields: vec![],
            post_effects: vec![],
            successors: vec![exit],
        }));

        // Emit entry instruction into the node
        self.instructions.push(Instruction::Match(MatchIR {
            label: entry,
            nav,
            node_type,
            node_field: None,
            pre_effects: vec![],
            neg_fields,
            post_effects: vec![],
            successors: vec![items_entry],
        }));

        entry
    }

    /// Compile an anonymous node: `"+"` or `_`.
    ///
    /// If `nav_override` is provided, uses that navigation instead of the default `Next`.
    fn compile_anonymous_node_with_nav(&mut self, node: &ast::AnonymousNode, exit: Label, nav_override: Option<Nav>) -> Label {
        let entry = self.fresh_label();
        let nav = nav_override.unwrap_or(Nav::Next);

        // Extract literal value (None for wildcard `_`)
        let node_type = node.value().and_then(|token| {
            let text = token.text();
            self.resolve_anonymous_node_type(text)
        });

        self.instructions.push(Instruction::Match(MatchIR {
            label: entry,
            nav,
            node_type,
            node_field: None,
            pre_effects: vec![],
            neg_fields: vec![],
            post_effects: vec![],
            successors: vec![exit],
        }));

        entry
    }

    /// Compile an anonymous node: `"+"` or `_`.
    fn compile_anonymous_node(&mut self, node: &ast::AnonymousNode, exit: Label) -> Label {
        self.compile_anonymous_node_with_nav(node, exit, None)
    }

    /// Compile a reference: `(Expr)`.
    fn compile_ref(&mut self, r: &ast::Ref, exit: Label) -> Label {
        self.compile_ref_with_nav(r, exit, None)
    }

    /// Compile a reference with optional navigation override.
    fn compile_ref_with_nav(&mut self, r: &ast::Ref, exit: Label, _nav_override: Option<Nav>) -> Label {
        let Some(name_token) = r.name() else {
            return exit;
        };
        let name = name_token.text();

        let Some(sym) = self.type_ctx.get_def_id(self.interner, name) else {
            return exit;
        };

        let Some(&target) = self.def_entries.get(&sym) else {
            return exit;
        };

        // Check if this is a recursive reference
        // Note: nav_override is not propagated here because the referenced
        // definition has its own navigation semantics
        if self.type_ctx.is_recursive(sym) {
            // Emit Call instruction
            let call_label = self.fresh_label();
            let ref_id = self.fresh_ref_id();

            self.instructions.push(Instruction::Call(CallIR {
                label: call_label,
                next: exit,
                target,
                ref_id,
            }));

            // Emit Return at the callee's exit
            let return_label = self.fresh_label();
            self.instructions.push(Instruction::Return(ReturnIR {
                label: return_label,
                ref_id,
            }));

            call_label
        } else {
            // Non-recursive: inline the body
            // For simplicity, just jump to the target
            target
        }
    }

    /// Compile a sequence: `{a b c}`.
    fn compile_seq(&mut self, seq: &ast::SeqExpr, exit: Label) -> Label {
        self.compile_seq_with_nav(seq, exit, None)
    }

    /// Compile a sequence with optional navigation override for first item.
    fn compile_seq_with_nav(&mut self, seq: &ast::SeqExpr, exit: Label, first_nav: Option<Nav>) -> Label {
        let items: Vec<_> = seq.items().collect();
        if items.is_empty() {
            return exit;
        }

        // Determine if we're inside a node based on the navigation override
        // Down variants mean we're descending into a node's children
        let is_inside_node = matches!(first_nav, Some(Nav::Down | Nav::DownSkip | Nav::DownExact));

        self.compile_seq_items_with_first_nav(&items, exit, is_inside_node, first_nav)
    }

    /// Compile an alternation: `[a b c]`.
    fn compile_alt(&mut self, alt: &ast::AltExpr, exit: Label) -> Label {
        self.compile_alt_with_nav(alt, exit, None)
    }

    /// Compile an alternation with optional navigation override for all branches.
    fn compile_alt_with_nav(&mut self, alt: &ast::AltExpr, exit: Label, first_nav: Option<Nav>) -> Label {
        let branches: Vec<_> = alt.branches().collect();
        if branches.is_empty() {
            return exit;
        }

        // Check if this is a tagged alternation (Enum type)
        let alt_expr = Expr::AltExpr(alt.clone());
        let is_enum = self
            .type_ctx
            .get_term_info(&alt_expr)
            .and_then(|info| info.flow.type_id())
            .and_then(|type_id| self.type_ctx.get_type(type_id))
            .is_some_and(|shape| matches!(shape, TypeShape::Enum(_)));

        // Compile each branch, collecting entry labels
        // Each branch gets the same nav override since any branch could match first
        let mut successors = Vec::new();
        for (variant_idx, branch) in branches.iter().enumerate() {
            let Some(body) = branch.body() else {
                continue;
            };

            if is_enum {
                // Tagged branch: E(variant_idx) → body → EndE → exit
                let ende_step = self.fresh_label();
                self.instructions.push(Instruction::Match(MatchIR {
                    label: ende_step,
                    nav: Nav::Stay,
                    node_type: None,
                    node_field: None,
                    pre_effects: vec![],
                    neg_fields: vec![],
                    post_effects: vec![EffectOp {
                        opcode: EffectOpcode::EndE,
                        payload: 0,
                    }],
                    successors: vec![exit],
                }));

                let body_entry = self.compile_expr_with_nav(&body, ende_step, first_nav);

                let e_step = self.fresh_label();
                self.instructions.push(Instruction::Match(MatchIR {
                    label: e_step,
                    nav: Nav::Stay,
                    node_type: None,
                    node_field: None,
                    pre_effects: vec![EffectOp {
                        opcode: EffectOpcode::E,
                        payload: variant_idx,
                    }],
                    neg_fields: vec![],
                    post_effects: vec![],
                    successors: vec![body_entry],
                }));

                successors.push(e_step);
            } else {
                // Untagged branch: direct compilation
                let branch_entry = self.compile_expr_with_nav(&body, exit, first_nav);
                successors.push(branch_entry);
            }
        }

        if successors.is_empty() {
            return exit;
        }
        if successors.len() == 1 {
            return successors[0];
        }

        // Emit epsilon branch to choose among alternatives
        let entry = self.fresh_label();
        self.emit_epsilon(entry, successors);
        entry
    }

    /// Compile a quantified expression: `a?`, `a*`, `a+`.
    fn compile_quantified(&mut self, quant: &ast::QuantifiedExpr, exit: Label) -> Label {
        self.compile_quantified_with_nav(quant, exit, None)
    }

    /// Compile a quantified expression with optional navigation override.
    fn compile_quantified_with_nav(
        &mut self,
        quant: &ast::QuantifiedExpr,
        exit: Label,
        nav_override: Option<Nav>,
    ) -> Label {
        let Some(inner) = quant.inner() else {
            return exit;
        };

        let Some(op) = quant.operator() else {
            return self.compile_expr_with_nav(&inner, exit, nav_override);
        };

        let is_plus = matches!(op.kind(), SyntaxKind::Plus | SyntaxKind::PlusQuestion);
        let is_star = matches!(op.kind(), SyntaxKind::Star | SyntaxKind::StarQuestion);
        let is_greedy = matches!(
            op.kind(),
            SyntaxKind::Question | SyntaxKind::Star | SyntaxKind::Plus
        );

        if is_plus {
            // +: body → loop_entry → [body, exit]
            // First iteration uses nav_override, subsequent use default
            let loop_entry = self.fresh_label();
            let first_body_entry = self.compile_expr_with_nav(&inner, loop_entry, nav_override);
            let repeat_body_entry = self.compile_expr(&inner, loop_entry);

            let successors = if is_greedy {
                vec![repeat_body_entry, exit]
            } else {
                vec![exit, repeat_body_entry]
            };
            self.emit_epsilon(loop_entry, successors);

            first_body_entry
        } else if is_star {
            // *: loop_entry → [body → loop_entry, exit]
            // First iteration uses nav_override, subsequent use default
            let loop_entry = self.fresh_label();
            let first_body_entry = self.compile_expr_with_nav(&inner, loop_entry, nav_override);
            let repeat_body_entry = self.compile_expr(&inner, loop_entry);

            // Entry point branches: first iteration or exit
            let entry = self.fresh_label();
            let successors = if is_greedy {
                vec![first_body_entry, exit]
            } else {
                vec![exit, first_body_entry]
            };
            self.emit_epsilon(entry, successors);

            // Loop point branches: repeat iteration or exit
            let loop_successors = if is_greedy {
                vec![repeat_body_entry, exit]
            } else {
                vec![exit, repeat_body_entry]
            };
            self.emit_epsilon(loop_entry, loop_successors);

            entry
        } else {
            // ?: branch to body or exit
            let body_entry = self.compile_expr_with_nav(&inner, exit, nav_override);
            let entry = self.fresh_label();

            let successors = if is_greedy {
                vec![body_entry, exit]
            } else {
                vec![exit, body_entry]
            };
            self.emit_epsilon(entry, successors);

            entry
        }
    }

    /// Compile a field constraint: `name: pattern`.
    fn compile_field(&mut self, field: &ast::FieldExpr, exit: Label) -> Label {
        self.compile_field_with_nav(field, exit, None)
    }

    /// Compile a field constraint with optional navigation override.
    fn compile_field_with_nav(
        &mut self,
        field: &ast::FieldExpr,
        exit: Label,
        nav_override: Option<Nav>,
    ) -> Label {
        let Some(value) = field.value() else {
            return exit;
        };

        let node_field = self.resolve_field(field);

        // Compile the value pattern with nav override
        let value_entry = self.compile_expr_with_nav(&value, exit, nav_override);

        // If we have a field constraint, wrap with a field-checking Match
        if node_field.is_some() {
            let entry = self.fresh_label();
            self.instructions.push(Instruction::Match(MatchIR {
                label: entry,
                nav: Nav::Stay, // Check field without moving
                node_type: None,
                node_field,
                pre_effects: vec![],
                neg_fields: vec![],
                post_effects: vec![],
                successors: vec![value_entry],
            }));
            return entry;
        }

        value_entry
    }

    /// Compile a captured expression: `@name` or `pattern @name`.
    fn compile_captured(&mut self, cap: &ast::CapturedExpr, exit: Label) -> Label {
        self.compile_captured_with_nav(cap, exit, None)
    }

    /// Compile a captured expression with optional navigation override.
    ///
    /// Effects are emitted AFTER the inner pattern matches, not before.
    /// Flow depends on captured type:
    /// - Scalar: inner_pattern → [Node/Text, Set] → exit
    /// - Struct: S → inner_pattern → [EndS, Node/Text, Set] → exit
    /// - Array:  A → quantifier (with Push) → [EndA, Node/Text, Set] → exit
    fn compile_captured_with_nav(
        &mut self,
        cap: &ast::CapturedExpr,
        exit: Label,
        nav_override: Option<Nav>,
    ) -> Label {
        // Determine effects based on capture type
        let is_text = cap.type_annotation().is_some_and(|t| {
            t.name().map(|n| n.text() == "string").unwrap_or(false)
        });

        // Build capture effects: Node/Text followed by Set(member_index)
        let mut capture_effects = Vec::with_capacity(2);

        let capture_effect = if is_text {
            EffectOp { opcode: EffectOpcode::Text, payload: 0 }
        } else {
            EffectOp { opcode: EffectOpcode::Node, payload: 0 }
        };
        capture_effects.push(capture_effect);

        // Add Set effect if we can resolve the member index
        if let Some(name_token) = cap.name()
            && let Some(member_idx) = self.lookup_member_index(name_token.text())
        {
            capture_effects.push(EffectOp {
                opcode: EffectOpcode::Set,
                payload: member_idx as usize,
            });
        }

        // Query type system to determine structural effects needed
        // - S/EndS: Check if INNER's flow is Bubble (has fields to collect)
        // - A/Push/EndA: Check if INNER's arity is Many (star/plus quantifier)
        let inner = cap.inner();
        let inner_info = inner.as_ref().and_then(|inner| self.type_ctx.get_term_info(inner));
        let inner_is_bubble = inner_info.map(|info| info.flow.is_bubble()).unwrap_or(false);

        // Check for Many arity (star/plus) - use type system if available, else syntactic check
        let inner_is_many = inner_info
            .map(|info| matches!(info.arity, Arity::Many))
            .unwrap_or_else(|| {
                // Fallback: check syntactically for star/plus
                inner.as_ref().is_some_and(|inner| {
                    if let Expr::QuantifiedExpr(q) = inner {
                        q.operator().is_some_and(|op| {
                            matches!(
                                op.kind(),
                                SyntaxKind::Star | SyntaxKind::StarQuestion | SyntaxKind::Plus | SyntaxKind::PlusQuestion
                            )
                        })
                    } else {
                        false
                    }
                })
            });

        if let Some(inner) = cap.inner() {
            // Pattern with capture: effects fire AFTER inner matches
            let effect_label = self.fresh_label();

            if inner_is_bubble {
                // Struct scope: S → inner → EndS + capture_effects → exit
                let mut post_effects = vec![EffectOp {
                    opcode: EffectOpcode::EndS,
                    payload: 0,
                }];
                post_effects.extend(capture_effects);

                self.instructions.push(Instruction::Match(MatchIR {
                    label: effect_label,
                    nav: Nav::Stay,
                    node_type: None,
                    node_field: None,
                    pre_effects: vec![],
                    neg_fields: vec![],
                    post_effects,
                    successors: vec![exit],
                }));

                let inner_entry = self.compile_expr_with_nav(&inner, effect_label, nav_override);

                // S step before inner
                let s_step = self.fresh_label();
                self.instructions.push(Instruction::Match(MatchIR {
                    label: s_step,
                    nav: Nav::Stay,
                    node_type: None,
                    node_field: None,
                    pre_effects: vec![EffectOp {
                        opcode: EffectOpcode::S,
                        payload: 0,
                    }],
                    neg_fields: vec![],
                    post_effects: vec![],
                    successors: vec![inner_entry],
                }));

                s_step
            } else if inner_is_many {
                // Array: A → quantifier (with Push) → EndA + capture_effects → exit
                let mut post_effects = vec![EffectOp {
                    opcode: EffectOpcode::EndA,
                    payload: 0,
                }];
                post_effects.extend(capture_effects);

                self.instructions.push(Instruction::Match(MatchIR {
                    label: effect_label,
                    nav: Nav::Stay,
                    node_type: None,
                    node_field: None,
                    pre_effects: vec![],
                    neg_fields: vec![],
                    post_effects,
                    successors: vec![exit],
                }));

                // Compile quantifier with Push effects
                let inner_entry = if let Expr::QuantifiedExpr(quant) = &inner {
                    self.compile_quantified_for_array(quant, effect_label, nav_override)
                } else {
                    self.compile_expr_with_nav(&inner, effect_label, nav_override)
                };

                // A step before inner
                let a_step = self.fresh_label();
                self.instructions.push(Instruction::Match(MatchIR {
                    label: a_step,
                    nav: Nav::Stay,
                    node_type: None,
                    node_field: None,
                    pre_effects: vec![EffectOp {
                        opcode: EffectOpcode::A,
                        payload: 0,
                    }],
                    neg_fields: vec![],
                    post_effects: vec![],
                    successors: vec![inner_entry],
                }));

                a_step
            } else {
                // Scalar: just capture effects
                self.instructions.push(Instruction::Match(MatchIR {
                    label: effect_label,
                    nav: Nav::Stay,
                    node_type: None,
                    node_field: None,
                    pre_effects: vec![],
                    neg_fields: vec![],
                    post_effects: capture_effects,
                    successors: vec![exit],
                }));

                self.compile_expr_with_nav(&inner, effect_label, nav_override)
            }
        } else {
            // Bare capture: just emit effects at current position
            let entry = self.fresh_label();
            self.instructions.push(Instruction::Match(MatchIR {
                label: entry,
                nav: Nav::Stay,
                node_type: None,
                node_field: None,
                pre_effects: vec![],
                neg_fields: vec![],
                post_effects: capture_effects,
                successors: vec![exit],
            }));
            entry
        }
    }

    /// Compile a quantified expression for array capture, adding Push after each iteration.
    fn compile_quantified_for_array(
        &mut self,
        quant: &ast::QuantifiedExpr,
        exit: Label,
        nav_override: Option<Nav>,
    ) -> Label {
        let Some(inner) = quant.inner() else {
            return exit;
        };

        let Some(op) = quant.operator() else {
            return self.compile_expr_with_nav(&inner, exit, nav_override);
        };

        let is_plus = matches!(op.kind(), SyntaxKind::Plus | SyntaxKind::PlusQuestion);
        let is_star = matches!(op.kind(), SyntaxKind::Star | SyntaxKind::StarQuestion);
        let is_greedy = matches!(
            op.kind(),
            SyntaxKind::Question | SyntaxKind::Star | SyntaxKind::Plus
        );

        // Push step: fires after each iteration completes
        let push_step = self.fresh_label();
        self.instructions.push(Instruction::Match(MatchIR {
            label: push_step,
            nav: Nav::Stay,
            node_type: None,
            node_field: None,
            pre_effects: vec![],
            neg_fields: vec![],
            post_effects: vec![EffectOp {
                opcode: EffectOpcode::Push,
                payload: 0,
            }],
            successors: vec![], // Will be set below
        }));
        let push_idx = self.instructions.len() - 1;

        if is_plus {
            // +: first_body → push → loop_entry → [repeat_body → push, exit]
            let loop_entry = self.fresh_label();

            // Bodies target push_step
            let first_body_entry = self.compile_expr_with_nav(&inner, push_step, nav_override);
            let repeat_body_entry = self.compile_expr(&inner, push_step);

            // Push leads to loop_entry
            if let Instruction::Match(ref mut m) = self.instructions[push_idx] {
                m.successors = vec![loop_entry];
            }

            // Loop chooses between repeat and exit
            let successors = if is_greedy {
                vec![repeat_body_entry, exit]
            } else {
                vec![exit, repeat_body_entry]
            };
            self.emit_epsilon(loop_entry, successors);

            first_body_entry
        } else if is_star {
            // *: entry → [first_body → push → loop_entry → [repeat_body → push, exit], exit]
            let loop_entry = self.fresh_label();

            // Bodies target push_step
            let first_body_entry = self.compile_expr_with_nav(&inner, push_step, nav_override);
            let repeat_body_entry = self.compile_expr(&inner, push_step);

            // Push leads to loop_entry
            if let Instruction::Match(ref mut m) = self.instructions[push_idx] {
                m.successors = vec![loop_entry];
            }

            // Entry point branches: first iteration or exit
            let entry = self.fresh_label();
            let successors = if is_greedy {
                vec![first_body_entry, exit]
            } else {
                vec![exit, first_body_entry]
            };
            self.emit_epsilon(entry, successors);

            // Loop point branches: repeat iteration or exit
            let loop_successors = if is_greedy {
                vec![repeat_body_entry, exit]
            } else {
                vec![exit, repeat_body_entry]
            };
            self.emit_epsilon(loop_entry, loop_successors);

            entry
        } else {
            // ?: branch to body or exit (no Push needed for optional)
            let body_entry = self.compile_expr_with_nav(&inner, exit, nav_override);
            let entry = self.fresh_label();

            let successors = if is_greedy {
                vec![body_entry, exit]
            } else {
                vec![exit, body_entry]
            };
            self.emit_epsilon(entry, successors);

            entry
        }
    }

    /// Emit an epsilon transition (no node interaction).
    fn emit_epsilon(&mut self, label: Label, successors: Vec<Label>) {
        self.instructions.push(Instruction::Match(MatchIR {
            label,
            nav: Nav::Stay,
            node_type: None,
            node_field: None,
            pre_effects: vec![],
            neg_fields: vec![],
            post_effects: vec![],
            successors,
        }));
    }

    /// Resolve an anonymous node's literal text to its node type ID.
    ///
    /// In linked mode, returns the grammar NodeTypeId for the literal.
    /// In unlinked mode, returns the StringId of the literal text.
    fn resolve_anonymous_node_type(&mut self, text: &str) -> Option<NonZeroU16> {
        if let Some(ids) = self.node_type_ids {
            // Linked mode: resolve to NodeTypeId from grammar
            for (&sym, &id) in ids {
                if self.interner.resolve(sym) == text {
                    return NonZeroU16::new(id.get());
                }
            }
            // If not found in grammar, treat as no constraint
            None
        } else {
            // Unlinked mode: store StringId referencing the literal text
            let string_id = self.strings.intern_str(text);
            NonZeroU16::new(string_id.0)
        }
    }

    /// Resolve a NamedNode to its node type ID.
    ///
    /// In linked mode, returns the grammar NodeTypeId.
    /// In unlinked mode, returns the StringId of the type name.
    fn resolve_node_type(&mut self, node: &ast::NamedNode) -> Option<NonZeroU16> {
        // For wildcard (_), no constraint
        if node.is_any() {
            return None;
        }

        let type_token = node.node_type()?;
        let type_name = type_token.text();

        if let Some(ids) = self.node_type_ids {
            // Linked mode: resolve to NodeTypeId from grammar
            for (&sym, &id) in ids {
                if self.interner.resolve(sym) == type_name {
                    return NonZeroU16::new(id.get());
                }
            }
            // If not found in grammar, treat as no constraint (linked mode)
            None
        } else {
            // Unlinked mode: store StringId referencing the type name
            let string_id = self.strings.intern_str(type_name);
            NonZeroU16::new(string_id.0)
        }
    }

    /// Resolve a field expression to its field ID.
    ///
    /// In linked mode, returns the grammar NodeFieldId.
    /// In unlinked mode, returns the StringId of the field name.
    fn resolve_field(&mut self, field: &ast::FieldExpr) -> Option<NonZeroU16> {
        let name_token = field.name()?;
        let field_name = name_token.text();
        self.resolve_field_by_name(field_name)
    }

    /// Resolve a field name to its field ID.
    ///
    /// In linked mode, returns the grammar NodeFieldId.
    /// In unlinked mode, returns the StringId of the field name.
    fn resolve_field_by_name(&mut self, field_name: &str) -> Option<NonZeroU16> {
        if let Some(ids) = self.node_field_ids {
            // Linked mode: resolve to NodeFieldId from grammar
            for (&sym, &id) in ids {
                if self.interner.resolve(sym) == field_name {
                    return NonZeroU16::new(id.get());
                }
            }
            // If not found in grammar, treat as no constraint (linked mode)
            None
        } else {
            // Unlinked mode: store StringId referencing the field name
            let string_id = self.strings.intern_str(field_name);
            NonZeroU16::new(string_id.0)
        }
    }

    /// Collect negated fields from a NamedNode.
    fn collect_neg_fields(&mut self, node: &ast::NamedNode) -> Vec<u16> {
        node.as_cst()
            .children()
            .filter_map(ast::NegatedField::cast)
            .filter_map(|nf| {
                let name = nf.name()?;
                self.resolve_field_by_name(name.text())
            })
            .map(|id| id.get())
            .collect()
    }

    /// Look up a capture name's member index in the current definition's type.
    ///
    /// Returns the position (0-based) of the field in the struct's BTreeMap order,
    /// or None if not found or not in a struct context.
    fn lookup_member_index(&self, capture_name: &str) -> Option<u16> {
        let def_id = self.current_def_id?;
        let type_id = self.type_ctx.get_def_type(def_id)?;
        let fields = self.type_ctx.get_struct_fields(type_id)?;

        // BTreeMap iterates in key order (Symbol order)
        for (index, (&sym, _)) in fields.iter().enumerate() {
            if self.interner.resolve(sym) == capture_name {
                return Some(index as u16);
            }
        }
        None
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
