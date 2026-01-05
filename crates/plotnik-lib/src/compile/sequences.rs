//! Sequence and alternation compilation.
//!
//! Handles compilation of:
//! - Sequences: `{a b c}` - siblings matched in order
//! - Alternations: `[a b c]` - first matching branch wins

use std::collections::BTreeMap;

use plotnik_core::Symbol;

use crate::analyze::type_check::{TypeId, TypeShape};
use crate::bytecode::ir::{EffectIR, Instruction, Label, MatchIR, MemberRef};
use crate::bytecode::{EffectOpcode, Nav};
use crate::parser::ast::{self, Expr, SeqItem};

use super::Compiler;
use super::capture::CaptureEffects;
use super::navigation::{compute_nav_modes, is_down_nav, is_skippable_quantifier, repeat_nav_for};

impl Compiler<'_> {
    /// Compile a sequence with capture effects (passed to last item).
    pub(super) fn compile_seq_inner(
        &mut self,
        seq: &ast::SeqExpr,
        exit: Label,
        first_nav: Option<Nav>,
        capture: CaptureEffects,
    ) -> Label {
        let items: Vec<_> = seq.items().collect();
        if items.is_empty() {
            return exit;
        }

        // Determine if we're inside a node based on the navigation override
        // Down variants mean we're descending into a node's children
        let is_inside_node = matches!(first_nav, Some(Nav::Down | Nav::DownSkip | Nav::DownExact));

        self.compile_seq_items_inner(&items, exit, is_inside_node, first_nav, capture, None)
    }

    /// Compile sequence items with capture effects (passed to last item).
    ///
    /// When `skip_exit` is provided, the skip path of the first skippable item
    /// will use this exit instead of `exit`. This is used when inside a node
    /// where skip paths should bypass the Up instruction.
    pub(super) fn compile_seq_items_inner(
        &mut self,
        items: &[SeqItem],
        exit: Label,
        is_inside_node: bool,
        first_nav: Option<Nav>,
        capture: CaptureEffects,
        skip_exit: Option<Label>,
    ) -> Label {
        // Compute navigation modes first (immutable borrow)
        let mut nav_modes = compute_nav_modes(items, is_inside_node);

        if nav_modes.is_empty() {
            return exit;
        }

        // Apply navigation to first expression
        let first_expr_nav = if let Some((_, first_mode)) = nav_modes.first_mut()
            && first_mode.is_none()
        {
            let nav = first_nav.or_else(|| is_inside_node.then_some(Nav::Down));
            *first_mode = nav;
            nav
        } else {
            nav_modes.first().and_then(|(_, n)| *n)
        };

        // Check if first expression is skippable and uses Down navigation.
        // In this case, the skip path needs different navigation than the match path.
        // Also trigger when skip_exit is provided (for bypassing Up in parent node).
        let first_is_skippable = items
            .first()
            .and_then(|item| item.as_expr())
            .is_some_and(is_skippable_quantifier);
        let first_uses_down = is_down_nav(first_expr_nav);
        let needs_skip_exit =
            first_is_skippable && first_uses_down && (nav_modes.len() > 1 || skip_exit.is_some());

        if needs_skip_exit {
            // Special handling: first item can skip and uses Down navigation.
            // The continuation needs two versions:
            // - skip_exit: uses Down nav (skipping means next item becomes "first")
            // - match_exit: uses Next nav (after matching, advance to sibling)
            return self.compile_seq_items_with_skip_exit(
                items,
                &nav_modes,
                exit,
                is_inside_node,
                first_expr_nav,
                capture,
                skip_exit,
            );
        }

        // Build chain in reverse: last expression exits to `exit`, each prior exits to next
        // Capture effects go to the FIRST iteration (which is the last expression)
        let mut current_exit = exit;
        let mut is_last = true;
        for (expr_idx, nav_override) in nav_modes.into_iter().rev() {
            let expr = items[expr_idx]
                .as_expr()
                .expect("nav_modes only contains expr indices");
            if is_last {
                // Last expression gets capture effects
                current_exit =
                    self.compile_expr_inner(expr, current_exit, nav_override, capture.clone());
                is_last = false;
            } else {
                current_exit = self.compile_expr_with_nav(expr, current_exit, nav_override);
            }
        }
        current_exit
    }

    /// Compile sequence items where first item is skippable and uses Down navigation.
    ///
    /// When the first item (optional/star) is skipped, the next item becomes the "first"
    /// and needs to use Down navigation instead of Next. This requires compiling the
    /// continuation twice with different navigation.
    ///
    /// When `caller_skip_exit` is provided and there are no remaining items, the skip
    /// path uses this exit (to bypass Up in parent node) while match path uses `exit`.
    #[allow(clippy::too_many_arguments)]
    fn compile_seq_items_with_skip_exit(
        &mut self,
        items: &[SeqItem],
        nav_modes: &[(usize, Option<Nav>)],
        exit: Label,
        is_inside_node: bool,
        first_nav: Option<Nav>,
        capture: CaptureEffects,
        caller_skip_exit: Option<Label>,
    ) -> Label {
        let first_expr = items[nav_modes[0].0]
            .as_expr()
            .expect("first item must be expression");

        // Build rest items once (shared between skip and match paths)
        let rest_items: Vec<_> = nav_modes[1..]
            .iter()
            .filter_map(|(idx, _)| items.get(*idx).and_then(|i| i.as_expr()))
            .map(|e| SeqItem::Expr(e.clone()))
            .collect();

        // Compile continuation with both navigations, or use exit if no continuation.
        // When caller_skip_exit is provided and rest is empty, use it for skip path
        // (this allows skip to bypass Up instruction in parent node).
        let (skip_exit, match_exit) = if rest_items.is_empty() {
            (caller_skip_exit.unwrap_or(exit), exit)
        } else {
            let skip = self.compile_seq_items_inner(
                &rest_items,
                exit,
                is_inside_node,
                first_nav, // Down variant for skip path
                capture.clone(),
                caller_skip_exit, // Propagate for nested skippables
            );
            let mtch = self.compile_seq_items_inner(
                &rest_items,
                exit,
                is_inside_node,
                repeat_nav_for(first_nav), // Next variant for match path
                capture.clone(),
                None, // Match path doesn't need skip exit
            );
            (skip, mtch)
        };

        self.compile_skippable_with_exits(first_expr, match_exit, skip_exit, first_nav, capture)
    }

    /// Compile an alternation with capture effects (passed to each branch).
    pub(super) fn compile_alt_inner(
        &mut self,
        alt: &ast::AltExpr,
        exit: Label,
        first_nav: Option<Nav>,
        capture: CaptureEffects,
    ) -> Label {
        let branches: Vec<_> = alt.branches().collect();
        if branches.is_empty() {
            return exit;
        }

        // Get alternation's type info
        let alt_expr = Expr::AltExpr(alt.clone());
        let alt_type_id = self
            .type_ctx
            .get_term_info(&alt_expr)
            .and_then(|info| info.flow.type_id());
        let alt_type_shape = alt_type_id.and_then(|id| self.type_ctx.get_type(id));

        // Check if THIS alternation is syntactically tagged (all branches have labels).
        // This is distinct from whether the type is an Enum - a nested untagged alternation
        // like `[[A: (x)]]` can inherit an enum type from its inner branch while the outer
        // branches have no labels.
        let is_tagged_alt = branches.iter().all(|b| b.label().is_some());
        let is_enum = is_tagged_alt
            && alt_type_shape.is_some_and(|shape| matches!(shape, TypeShape::Enum(_)));

        // For tagged alternations: build map from label Symbol to (member index, payload TypeId)
        // This ensures we use the correct BTreeMap order indices, not AST iteration order
        let variant_info: BTreeMap<Symbol, (u16, TypeId)> = match alt_type_shape {
            Some(TypeShape::Enum(variants)) => variants
                .iter()
                .enumerate()
                .map(|(idx, (&sym, &type_id))| (sym, (idx as u16, type_id)))
                .collect(),
            _ => BTreeMap::new(),
        };
        let merged_fields = alt_type_id.and_then(|id| self.type_ctx.get_struct_fields(id));

        // Convert navigation to exact variant for alternation branches.
        // Branches should match at their exact cursor position only -
        // the search among positions is owned by the parent context.
        // When first_nav is None (standalone definition), use StayExact.
        let branch_nav = Some(first_nav.map_or(Nav::StayExact, Nav::to_exact));

        // Compile each branch, collecting entry labels
        let mut successors = Vec::new();
        for branch in branches.iter() {
            let Some(body) = branch.body() else {
                continue;
            };

            if is_enum {
                // Look up variant info by branch label (using BTreeMap order, not AST order)
                let label = branch.label().expect("tagged branch must have label");
                let label_text = label.text();
                let (variant_idx, payload_type_id) = variant_info
                    .iter()
                    .find(|(sym, _)| self.interner.resolve(**sym) == label_text)
                    .map(|(_, &(idx, type_id))| (idx, type_id))
                    .expect("variant must exist for labeled branch");

                // Tagged branch: E(variant_ref) → body → EndE → exit
                // Outer capture effects go on EndEnum, not on the branch body
                let mut end_effects = vec![EffectIR::simple(EffectOpcode::EndEnum, 0)];
                end_effects.extend(capture.post.iter().cloned());

                let ende_step = self.fresh_label();
                self.instructions.push(Instruction::Match(MatchIR {
                    label: ende_step,
                    nav: Nav::Stay,
                    node_type: None,
                    node_field: None,
                    pre_effects: vec![],
                    neg_fields: vec![],
                    post_effects: end_effects,
                    successors: vec![exit],
                }));

                // Compile body with variant's scope (no outer capture - it's on EndEnum)
                let body_entry = self.with_scope(payload_type_id, |this| {
                    this.compile_expr_inner(&body, ende_step, branch_nav, CaptureEffects::default())
                });

                // Create deferred member reference for the enum variant
                // Uses parent enum type + relative index (enum variants don't bubble)
                let e_effect = if let Some(type_id) = alt_type_id {
                    EffectIR::with_member(
                        EffectOpcode::Enum,
                        MemberRef::deferred_by_index(type_id, variant_idx),
                    )
                } else {
                    // Fallback: use absolute index (should not happen for well-formed queries)
                    EffectIR::simple(EffectOpcode::Enum, 0)
                };

                let e_step = self.fresh_label();
                self.instructions.push(Instruction::Match(MatchIR {
                    label: e_step,
                    nav: Nav::Stay,
                    node_type: None,
                    node_field: None,
                    pre_effects: vec![e_effect],
                    neg_fields: vec![],
                    post_effects: vec![],
                    successors: vec![body_entry],
                }));

                successors.push(e_step);
            } else {
                // Untagged branch: compile body, then inject Null for missing captures
                let branch_entry =
                    self.compile_expr_inner(&body, exit, branch_nav, capture.clone());

                let Some(fields) = merged_fields else {
                    successors.push(branch_entry);
                    continue;
                };
                let Some(alt_type) = alt_type_id else {
                    successors.push(branch_entry);
                    continue;
                };

                // Build Null effects for fields not captured by this branch
                let branch_captures = Self::collect_captures(&body);
                let null_effects: Vec<_> = fields
                    .iter()
                    .enumerate()
                    .filter(|(_, (sym, _))| !branch_captures.contains(self.interner.resolve(**sym)))
                    .flat_map(|(idx, _)| {
                        [
                            EffectIR::simple(EffectOpcode::Null, 0),
                            EffectIR::with_member(
                                EffectOpcode::Set,
                                MemberRef::deferred_by_index(alt_type, idx as u16),
                            ),
                        ]
                    })
                    .collect();

                if null_effects.is_empty() {
                    successors.push(branch_entry);
                    continue;
                }

                let null_step = self.fresh_label();
                self.instructions.push(Instruction::Match(MatchIR {
                    label: null_step,
                    nav: Nav::Stay,
                    node_type: None,
                    node_field: None,
                    pre_effects: null_effects,
                    neg_fields: vec![],
                    post_effects: vec![],
                    successors: vec![branch_entry],
                }));
                successors.push(null_step);
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
}
