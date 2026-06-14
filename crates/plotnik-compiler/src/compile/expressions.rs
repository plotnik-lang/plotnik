//! Expression compilation for leaf and wrapper patterns.
//!
//! Handles compilation of:
//! - Named nodes: `(identifier)`, `(call_expression ...)`
//! - Anonymous nodes: `"+"`, `_`
//! - References: `(Expr)` (calls to other definitions)
//! - Field constraints: `name: pattern`
//! - Captured expressions: `@name`, `pattern @name`

use std::num::NonZeroU16;

use crate::analyze::type_check::TypeShape;
use crate::bytecode::{EffectIR, InstructionIR, Label, MatchIR, NodeTypeIR, PredicateIR};
use crate::parser::ast::{self, Expr};
use plotnik_bytecode::Nav;
use plotnik_core::NodeType;

use crate::analyze::type_check::{CaptureMechanism, capture_mechanism};

use super::Compiler;
use super::capture::CaptureEffects;
use super::navigation::check_trailing_anchor;
use super::sequences::SeqItemsCtx;

/// Parameters for compiling a named node whose body ends in a trailing anchor.
///
/// Bundles the parent-node match envelope (`entry`/`exit`/`nav`/`node_type`/
/// `neg_fields`/`predicate`/`capture`), the resolved Up navigation, and the
/// borrowed body items into one descriptor for the skip-retry emission.
struct NamedNodeTrailingCtx<'a> {
    entry: Label,
    exit: Label,
    nav: Nav,
    node_type: NodeTypeIR,
    neg_fields: Vec<u16>,
    predicate: Option<PredicateIR>,
    items: &'a [ast::SeqItem],
    up_nav: Nav,
    capture: CaptureEffects,
}

impl Compiler<'_> {
    /// Compile a named node with capture effects.
    pub(super) fn compile_named_node_inner(
        &mut self,
        node: &ast::NamedNode,
        exit: Label,
        nav_override: Option<Nav>,
        capture: CaptureEffects,
    ) -> Label {
        let entry = self.fresh_label();
        let node_type = self.resolve_node_type(node);
        let nav = nav_override.unwrap_or(Nav::Stay);

        // Collect items, negated fields, and predicate
        let items: Vec<_> = node.items().collect();
        let neg_fields = self.collect_neg_fields(node);
        let predicate = self.compile_predicate(node);

        // If no items, just match and exit (capture effects go here)
        if items.is_empty() {
            let mut m = MatchIR::epsilon(entry, exit)
                .nav(nav)
                .node_type(node_type)
                .neg_fields(neg_fields)
                .pre_effects(capture.pre)
                .post_effects(capture.post);
            if let Some(p) = predicate {
                m = m.predicate(p);
            }
            return self.emit_match(m);
        }

        // Determine Up navigation based on trailing anchor
        let (has_trailing_anchor, trailing_nav) =
            check_trailing_anchor(&items, self.ctx.symbol_table);

        // Emit Up instruction with appropriate strictness
        let up_nav = if has_trailing_anchor {
            trailing_nav.unwrap_or(Nav::UpSkipTrivia(1))
        } else {
            Nav::Up(1)
        };

        // Trailing anchor requires skip-retry pattern for backtracking.
        // When the anchor check fails (matched node is not last), we need to
        // retry with the next sibling until we find one that IS last.
        if has_trailing_anchor {
            return self.compile_named_node_with_trailing_anchor(NamedNodeTrailingCtx {
                entry,
                exit,
                nav,
                node_type,
                neg_fields,
                predicate,
                items: &items,
                up_nav,
                capture,
            });
        }

        // Split capture.post: Node/Text effects (and their Set) go on entry (need matched_node
        // right after match), other effects go on final_exit (after children processing).
        // Node/Text capture effects use matched_node which is only valid immediately after the match,
        // before descending into children (which may clobber matched_node via backtracking).
        use plotnik_bytecode::EffectOpcode;

        // Find Node/Text effects and their following Set effects (they come in pairs: Node Set)
        let mut entry_effects = Vec::new();
        let mut exit_effects = Vec::new();
        let mut iter = capture.post.into_iter().peekable();
        while let Some(eff) = iter.next() {
            if matches!(eff.opcode(), EffectOpcode::Node | EffectOpcode::Text) {
                entry_effects.push(eff);
                // Take the following Set if present (Node/Text are always followed by Set)
                if iter.peek().is_some_and(|e| e.opcode() == EffectOpcode::Set) {
                    entry_effects.push(iter.next().unwrap());
                }
            } else {
                exit_effects.push(eff);
            }
        }

        // With items: nav[entry_effects] → items → Up → [exit_effects] → exit
        let final_exit = self.emit_post_effects_exit(exit, exit_effects);

        let up_label = self.fresh_label();
        let items_entry = self.compile_seq_items_inner(SeqItemsCtx {
            items: &items,
            exit: up_label,
            is_inside_node: true,
            first_nav: None,
            capture: CaptureEffects::default(),
            skip_exit: Some(final_exit), // Skip exit bypasses Up when Down fails (childless node)
        });

        self.instructions
            .push(MatchIR::epsilon(up_label, final_exit).nav(up_nav).into());

        // Emit entry instruction with node_effects on post (executes right after match)
        let mut entry_match = MatchIR::epsilon(entry, items_entry)
            .nav(nav)
            .node_type(node_type)
            .neg_fields(neg_fields)
            .pre_effects(capture.pre)
            .post_effects(entry_effects);
        if let Some(p) = predicate {
            entry_match = entry_match.predicate(p);
        }
        self.emit_match(entry_match);

        entry
    }

    /// Compile a named node with trailing anchor using skip-retry pattern.
    ///
    /// Structure:
    /// ```text
    /// entry: Match(nav, node_type) → down_wildcard
    /// down_wildcard: Match(Down, wildcard) → try
    /// try: epsilon → [body, retry_nav]
    /// body: items (StayExact) → up_check
    /// up_check: Match(up_nav, None) → exit
    /// retry_nav: Match(Next, wildcard) → try
    /// ```
    ///
    /// When items match but the trailing anchor check fails, we backtrack to `try`,
    /// which falls through to `retry_nav`, advances to next sibling, and retries.
    /// Only when siblings are exhausted does backtracking propagate to the caller.
    fn compile_named_node_with_trailing_anchor(&mut self, ctx: NamedNodeTrailingCtx<'_>) -> Label {
        let NamedNodeTrailingCtx {
            entry,
            exit,
            nav,
            node_type,
            neg_fields,
            predicate,
            items,
            up_nav,
            capture,
        } = ctx;

        let final_exit = self.emit_post_effects_exit(exit, capture.post);

        // up_check: Match(up_nav) → final_exit
        let up_check = self.fresh_label();
        self.instructions
            .push(MatchIR::epsilon(up_check, final_exit).nav(up_nav).into());

        // body: items with StayExact navigation → up_check
        // Items are compiled with StayExact because the skip-retry loop handles
        // advancement; the body should match at the current position only.
        let body = self.compile_seq_items_inner(SeqItemsCtx {
            items,
            exit: up_check,
            is_inside_node: true,
            first_nav: Some(Nav::StayExact), // First item uses StayExact (we're already at position)
            capture: CaptureEffects::default(),
            skip_exit: None,
        });

        // The node's children are searched with a resumable position search:
        // an adjacency failure at the trailing anchor (`up_check`) retries at the
        // next child instead of failing the whole match.
        let down_wildcard = self.emit_position_search(Nav::Down, body);

        // entry: match parent node → down_wildcard (only pre_effects here)
        let mut entry_match = MatchIR::epsilon(entry, down_wildcard)
            .nav(nav)
            .node_type(node_type)
            .neg_fields(neg_fields)
            .pre_effects(capture.pre);
        if let Some(p) = predicate {
            entry_match = entry_match.predicate(p);
        }
        self.emit_match(entry_match);

        entry
    }

    /// Emit post-effects on an epsilon step after the exit label.
    ///
    /// Post-effects (like EndEnum) must execute AFTER children complete, not after
    /// matching the parent node. This helper creates an epsilon step for the effects
    /// when needed, or returns the original exit if no effects.
    fn emit_post_effects_exit(&mut self, exit: Label, post: Vec<EffectIR>) -> Label {
        if post.is_empty() {
            exit
        } else {
            self.emit_effects_epsilon(exit, post, CaptureEffects::default())
        }
    }

    /// Compile an anonymous node with capture effects.
    pub(super) fn compile_anonymous_node_inner(
        &mut self,
        node: &ast::AnonymousNode,
        exit: Label,
        nav_override: Option<Nav>,
        capture: CaptureEffects,
    ) -> Label {
        let entry = self.fresh_label();
        let nav = nav_override.unwrap_or(Nav::Next);

        // Extract literal value (Any for wildcard `_`, Anonymous for literals)
        let node_type = match node.value() {
            Some(token) => self.resolve_anonymous_node_type(token.text()),
            None => NodeTypeIR::Any, // `_` wildcard matches any node
        };

        self.emit_match(
            MatchIR::epsilon(entry, exit)
                .nav(nav)
                .node_type(node_type)
                .pre_effects(capture.pre)
                .post_effects(capture.post),
        );

        entry
    }

    /// Compile a reference with capture effects.
    ///
    /// Call-site scoping: the caller decides whether to wrap with Obj/EndObj based on
    /// whether the ref is captured and the called definition returns a struct.
    ///
    /// - Captured ref returning struct: `Obj → Call → EndObj → Set → exit`
    /// - Captured ref returning scalar: `Call → Set → exit`
    /// - Uncaptured ref: `Call → exit` (def's Sets go to parent scope)
    pub(super) fn compile_ref_inner(
        &mut self,
        r: &ast::Ref,
        exit: Label,
        nav_override: Option<Nav>,
        field_override: Option<NonZeroU16>,
        capture: CaptureEffects,
    ) -> Label {
        let Some(name_token) = r.name() else {
            return exit;
        };
        let name = name_token.text();

        let Some(def_id) = self.ctx.type_ctx.get_def_id(self.ctx.interner, name) else {
            return exit;
        };

        let Some(&target) = self.def_entries.get(&def_id) else {
            return exit;
        };

        // Check if the called definition returns a struct (needs scope isolation when captured)
        let def_type_id = self.ctx.type_ctx.get_def_type(def_id);
        let ref_returns_struct = def_type_id
            .and_then(|tid| self.ctx.type_ctx.get_type(tid))
            .is_some_and(|shape| matches!(shape, TypeShape::Struct(_)));

        // Determine if this is a captured ref that needs scope isolation
        let is_captured = !capture.post.is_empty();
        let needs_scope = is_captured && ref_returns_struct;

        // An uncaptured recursive reference is opaque: inference types it Void, so
        // its captures must not bubble into the parent scope. Tagged-union recursion
        // is the exception — inference forwards its enum value — so only the rest is
        // suppressed.
        let ref_returns_enum = def_type_id
            .and_then(|tid| self.ctx.type_ctx.get_type(tid))
            .is_some_and(|shape| matches!(shape, TypeShape::Enum(_)));
        let suppress_opaque_recursion =
            !is_captured && self.ctx.type_ctx.is_recursive(def_id) && !ref_returns_enum;

        let nav = nav_override.unwrap_or(Nav::Stay);

        // Call instructions don't have pre_effects, so emit epsilon if needed
        let call_entry = if needs_scope {
            // Captured ref returning struct: Obj → Call → EndObj → Set → exit
            // The Obj creates an isolated scope for the definition's internal captures.
            let set_step = self.emit_effects_epsilon(exit, capture.post, CaptureEffects::default());
            let endobj_step = self.emit_endobj_step(set_step);
            let call_label = self.emit_call(nav, field_override, endobj_step, target);
            self.emit_obj_step(call_label)
        } else if is_captured {
            // Captured ref returning scalar: Call → Set → exit
            let return_addr =
                self.emit_effects_epsilon(exit, capture.post, CaptureEffects::default());
            self.emit_call(nav, field_override, return_addr, target)
        } else if suppress_opaque_recursion {
            // Uncaptured opaque recursion: SuppressBegin → Call → SuppressEnd → exit.
            // The recursion still matches structurally but contributes no effects,
            // matching its inferred Void type.
            let suppress_end = self.emit_effects_epsilon(
                exit,
                vec![EffectIR::suppress_end()],
                CaptureEffects::default(),
            );
            let call_label = self.emit_call(nav, field_override, suppress_end, target);
            self.emit_effects_epsilon(
                call_label,
                vec![EffectIR::suppress_begin()],
                CaptureEffects::default(),
            )
        } else {
            // Uncaptured ref: just Call → exit (def's Sets go to parent scope)
            self.emit_call(nav, field_override, exit, target)
        };

        if capture.pre.is_empty() {
            return call_entry;
        }

        // Wrap with pre-effects epsilon (e.g., Enum for tagged alternations)
        self.emit_effects_epsilon(call_entry, capture.pre, CaptureEffects::default())
    }

    /// Compile a field constraint with capture effects (passed to inner pattern).
    pub(super) fn compile_field_inner(
        &mut self,
        field: &ast::FieldExpr,
        exit: Label,
        nav_override: Option<Nav>,
        capture: CaptureEffects,
    ) -> Label {
        let Some(value) = field.value() else {
            return exit;
        };

        let node_field = self.resolve_field(field);

        // Special case: if value is a reference, pass field and capture to Call instruction
        if let Expr::Ref(r) = &value {
            return self.compile_ref_inner(r, exit, nav_override, node_field, capture);
        }

        // Check if value is a complex pattern (alternation, sequence, quantified)
        // that will produce an epsilon branch and thus need a wrapper instruction.
        // For these patterns, we must:
        // 1. Compile the value WITHOUT navigation (branches use Stay)
        // 2. Create a wrapper WITH navigation AND field check
        // This ensures the field is checked AFTER navigating to the child node.
        let needs_wrapper = node_field.is_some()
            && matches!(
                &value,
                Expr::AltExpr(_) | Expr::SeqExpr(_) | Expr::QuantifiedExpr(_)
            );

        if needs_wrapper {
            // Compile value WITHOUT navigation - wrapper will handle it
            let value_entry = self.compile_expr_inner(&value, exit, None, capture);

            let entry = self.fresh_label();
            self.instructions.push(
                MatchIR::epsilon(entry, value_entry)
                    .nav(nav_override.unwrap_or(Nav::Stay))
                    .node_field(node_field)
                    .into(),
            );
            return entry;
        }

        // Simple pattern: compile with navigation, merge field afterward
        let value_entry = self.compile_expr_inner(&value, exit, nav_override, capture);

        // If we have a field constraint, try to merge it into the value's instruction
        if let Some(field_id) = node_field {
            // Try to find and merge with the instruction we just created
            if let Some(instr) = self
                .instructions
                .iter_mut()
                .find(|i| i.label() == value_entry)
                && let InstructionIR::Match(m) = instr
                && m.node_field.is_none()
            {
                m.node_field = Some(field_id);
                return value_entry;
            }

            // Fallback: wrap with field-checking Match for patterns that couldn't merge.
            // Use Stay since value was already compiled with navigation.
            let entry = self.fresh_label();
            self.instructions.push(
                MatchIR::epsilon(entry, value_entry)
                    .node_field(field_id)
                    .into(),
            );
            return entry;
        }

        value_entry
    }

    /// Compile a captured expression with capture effects from outer layers.
    ///
    /// Capture effects are placed on the innermost match instruction:
    /// - Scalar: inner_pattern[Node/Text, Set] → exit
    /// - Struct: Obj epsilon → inner_pattern[Node/Text, Set] → EndObj epsilon → exit
    /// - Array:  Arr epsilon → quantifier (with Push on body) → EndArr+Set epsilon → exit
    /// - Ref:    Call → Set epsilon → exit (structured result needs epsilon)
    /// - Suppressive: SuppressBegin → inner → SuppressEnd → outer_effects → exit
    pub(super) fn compile_captured_inner(
        &mut self,
        cap: &ast::CapturedExpr,
        exit: Label,
        nav_override: Option<Nav>,
        outer_capture: CaptureEffects,
    ) -> Label {
        // Handle suppressive captures: wrap inner with SuppressBegin/End
        if cap.is_suppressive() {
            return self.compile_suppressive_capture(cap, exit, nav_override, outer_capture);
        }

        let inner = cap.inner();
        let inner_info = inner
            .as_ref()
            .and_then(|i| self.ctx.type_ctx.get_term_info(i));
        let inner_is_bubble = inner_info
            .as_ref()
            .is_some_and(|info| info.flow.is_bubble());

        // Build capture effects: [Node/Text] (only for the Node mechanism) + Set.
        let capture_effects = self.build_capture_effects(cap, inner.as_ref());

        // Bare capture: just emit effects at current position
        let Some(inner) = inner else {
            return self.emit_effects_epsilon(exit, capture_effects, outer_capture);
        };

        // The classifier is the single source of truth shared with inference, so
        // the effects we emit here always match the type that was declared.
        match capture_mechanism(&inner, self.ctx.type_ctx, self.ctx.interner) {
            // Array: Arr → quantifier (with Push) → EndArr+capture → exit
            CaptureMechanism::Array => self.compile_array_scope(
                &inner,
                exit,
                nav_override,
                capture_effects,
                outer_capture,
                cap.has_string_annotation(),
            ),

            // Struct scope: Obj → inner → EndObj+capture → exit (also empty `{}`).
            CaptureMechanism::StructScope => {
                let scope_type_id = inner_info.as_ref().and_then(|info| info.flow.type_id());
                self.compile_struct_scope(
                    &inner,
                    exit,
                    nav_override,
                    scope_type_id,
                    capture_effects,
                    outer_capture,
                )
            }

            // Set-after: the inner leaves the value pending (tagged alternation or a
            // named node forwarding a structured child). Emit the inner, then a
            // trailing Set; no Node, no wrapper.
            CaptureMechanism::SetAfter => {
                let CaptureEffects { pre, post } = outer_capture;
                let set_step = self.emit_effects_epsilon(
                    exit,
                    capture_effects,
                    CaptureEffects::new_post(post),
                );
                let inner_entry = self.compile_expr_inner(
                    &inner,
                    set_step,
                    nav_override,
                    CaptureEffects::default(),
                );
                // The enclosing variant's `Enum`-open (in `pre`) must run before the
                // inner produces its pending value; routing it through the trailing
                // `Set` step would drop it and unbalance the scope.
                self.wrap_entry_pre(inner_entry, pre)
            }

            // Ref: hand the capture to the call site, which wraps Call/Return (and
            // Obj/EndObj for struct-returning definitions) to isolate the
            // definition's internal captures before the Set.
            CaptureMechanism::Ref => {
                let combined = outer_capture.with_post_values(capture_effects);
                self.compile_expr_inner(&inner, exit, nav_override, combined)
            }

            // Node: capture the matched node. Bubbling children, if any, set into
            // the current scope alongside the capture.
            CaptureMechanism::Node => {
                if inner_is_bubble {
                    self.compile_bubble_with_node_capture(
                        &inner,
                        exit,
                        nav_override,
                        None,
                        capture_effects,
                        outer_capture,
                    )
                } else {
                    let combined = outer_capture.with_post_values(capture_effects);
                    self.compile_expr_inner(&inner, exit, nav_override, combined)
                }
            }
        }
    }

    /// Compile a suppressive capture (@_ or @_name).
    ///
    /// Suppressive captures match structurally but don't emit effects.
    /// Flow: outer.pre → SuppressBegin → inner → SuppressEnd → outer.post → exit
    fn compile_suppressive_capture(
        &mut self,
        cap: &ast::CapturedExpr,
        exit: Label,
        nav_override: Option<Nav>,
        outer_capture: CaptureEffects,
    ) -> Label {
        let CaptureEffects { pre, post } = outer_capture;

        let Some(inner) = cap.inner() else {
            // Bare @_ with no inner - just pass through outer effects.
            if pre.is_empty() && post.is_empty() {
                return exit;
            }
            let entry = self.emit_effects_epsilon(exit, vec![], CaptureEffects::new_post(post));
            return self.wrap_entry_pre(entry, pre);
        };

        // SuppressEnd + outer post (e.g. a tagged variant's EndEnum) → exit
        let suppress_end = vec![EffectIR::suppress_end()];
        let end_label =
            self.emit_effects_epsilon(exit, suppress_end, CaptureEffects::new_post(post));

        // Compile inner → end_label (inner gets NO capture effects)
        let inner_entry =
            self.compile_expr_inner(&inner, end_label, nav_override, CaptureEffects::default());

        // SuppressBegin → inner_entry
        let suppress_begin = vec![EffectIR::suppress_begin()];
        let begin_entry =
            self.emit_effects_epsilon(inner_entry, suppress_begin, CaptureEffects::default());

        // outer `pre` (the variant's `Enum`-open) runs before SuppressBegin, in the
        // enclosing scope — so the tag is produced and the later `EndEnum` matches.
        self.wrap_entry_pre(begin_entry, pre)
    }

    /// Resolve an anonymous node's literal text to its node type constraint.
    ///
    /// Returns `NodeTypeIR::Anonymous` with the type ID.
    pub(super) fn resolve_anonymous_node_type(&mut self, text: &str) -> NodeTypeIR {
        if let Some(ids) = self.ctx.node_types {
            // Linked mode: resolve to NodeTypeId from grammar
            let Some(sym) = self.ctx.interner.get(text) else {
                return NodeTypeIR::Anonymous(None);
            };
            ids.get(&NodeType::Anonymous(sym))
                .and_then(|id| NonZeroU16::new(id.get()))
                .map_or(NodeTypeIR::Anonymous(None), |id| {
                    NodeTypeIR::Anonymous(Some(id))
                })
        } else {
            // Unlinked mode: store StringId referencing the literal text
            let string_id = self.ctx.strings.borrow_mut().intern_str(text);
            NodeTypeIR::Anonymous(Some(string_id.0))
        }
    }

    /// Resolve a NamedNode to its node type constraint.
    ///
    /// Returns `NodeTypeIR::Named` with:
    /// - `None` for wildcard `(_)` (any named node)
    /// - `Some(id)` for specific types like `(identifier)`
    pub(super) fn resolve_node_type(&mut self, node: &ast::NamedNode) -> NodeTypeIR {
        // For wildcard (_), return Named(None) for "any named node"
        if node.is_any() {
            return NodeTypeIR::Named(None);
        }

        let Some(type_token) = node.node_type() else {
            // No type specified - treat as any named
            return NodeTypeIR::Named(None);
        };
        let type_name = type_token.text();

        if let Some(ids) = self.ctx.node_types {
            // Linked mode: resolve to NodeTypeId from grammar
            let Some(sym) = self.ctx.interner.get(type_name) else {
                return NodeTypeIR::Named(None);
            };
            ids.get(&NodeType::Named(sym))
                .and_then(|id| NonZeroU16::new(id.get()))
                .map_or(NodeTypeIR::Named(None), |id| NodeTypeIR::Named(Some(id)))
        } else {
            // Unlinked mode: store StringId referencing the type name
            let string_id = self.ctx.strings.borrow_mut().intern_str(type_name);
            NodeTypeIR::Named(Some(string_id.0))
        }
    }

    /// Resolve a field expression to its field ID.
    ///
    /// In linked mode, returns the grammar NodeFieldId.
    /// In unlinked mode, returns the StringId of the field name.
    pub(super) fn resolve_field(&mut self, field: &ast::FieldExpr) -> Option<NonZeroU16> {
        let name_token = field.name()?;
        let field_name = name_token.text();
        self.resolve_field_by_name(field_name)
    }

    /// Resolve a field name to its field ID.
    ///
    /// In linked mode, returns the grammar NodeFieldId.
    /// In unlinked mode, returns the StringId of the field name.
    pub(super) fn resolve_field_by_name(&mut self, field_name: &str) -> Option<NonZeroU16> {
        if let Some(ids) = self.ctx.node_fields {
            // Linked mode: an unknown field is left unconstrained.
            self.ctx
                .interner
                .get(field_name)
                .and_then(|sym| ids.get(&sym))
                .and_then(|id| NonZeroU16::new(id.get()))
        } else {
            // Unlinked mode: store StringId referencing the field name
            let string_id = self.ctx.strings.borrow_mut().intern_str(field_name);
            Some(string_id.0)
        }
    }

    /// Collect negated fields from a NamedNode.
    pub(super) fn collect_neg_fields(&mut self, node: &ast::NamedNode) -> Vec<u16> {
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

    /// Compile a predicate from AST to IR.
    ///
    /// Returns `Some(PredicateIR)` if the node has a valid predicate, `None` otherwise.
    pub(super) fn compile_predicate(&mut self, node: &ast::NamedNode) -> Option<PredicateIR> {
        let pred = node.predicate()?;
        let op = pred.operator()?;

        // Try string value first
        if let Some(str_token) = pred.string_value() {
            let string_id = self.ctx.strings.borrow_mut().intern_str(str_token.text());
            return Some(PredicateIR::string(op, string_id));
        }

        // Try regex value
        if let Some(regex) = pred.regex() {
            // Get pattern text by stripping `/` delimiters from CST text
            let text: String = regex.as_cst().text().into();
            let without_prefix = text.strip_prefix('/').unwrap_or(&text);
            let pattern = without_prefix.strip_suffix('/').unwrap_or(without_prefix);
            let string_id = self.ctx.strings.borrow_mut().intern_str(pattern);
            return Some(PredicateIR::regex(op, string_id));
        }

        None
    }
}
