//! Sequence and alternation compilation.
//!
//! Handles compilation of:
//! - Sequences: `{a b c}` - siblings matched in order
//! - Alternations: `[a b c]` - first matching branch wins

use std::collections::BTreeMap;

use plotnik_core::Symbol;

use crate::analyze::type_check::{TypeId, TypeShape};
use crate::bytecode::{EffectIR, Label, MemberRef};
use crate::parser::ast::{self, Expr, SeqItem};
use plotnik_bytecode::{EffectOpcode, Nav};

use super::Compiler;
use super::capture::CaptureEffects;
use super::navigation::{
    AnonymousClassifier, compute_nav_modes, expr_owns_iteration, is_down_nav,
    is_skippable_quantifier,
};

/// Extract the navigation if it's a resumable sibling search (`SkipPolicy::Any`).
/// Bounded navs (anchored, exact) have deterministic candidates and need no
/// search wrapper; Up navs don't search siblings at all.
fn resumable_search_nav(first_nav: Option<Nav>) -> Option<Nav> {
    match first_nav {
        Some(nav @ (Nav::Down | Nav::Next | Nav::Stay)) => Some(nav),
        _ => None,
    }
}

fn exact_nav_for_alt_branch(first_nav: Option<Nav>, search_nav: Option<Nav>) -> Option<Nav> {
    if search_nav.is_some() {
        return Some(Nav::StayExact);
    }

    let nav = match first_nav {
        None => Nav::StayExact,
        Some(
            nav @ (Nav::DownSkip
            | Nav::DownSkipExtras
            | Nav::NextSkip
            | Nav::NextSkipExtras
            | Nav::UpSkipTrivia(_)
            | Nav::UpSkipExtras(_)),
        ) => nav,
        Some(nav) => nav.to_exact(),
    };
    Some(nav)
}

fn nav_for_alt_branch(
    first_nav: Option<Nav>,
    search_nav: Option<Nav>,
    body: &Expr,
    classifier: &AnonymousClassifier<'_>,
) -> Option<Nav> {
    let nav = exact_nav_for_alt_branch(first_nav, search_nav)?;

    if !classifier.expr_may_match_anonymous(Some(body)) {
        return Some(nav);
    }

    Some(match nav {
        Nav::DownSkip => Nav::DownSkipExtras,
        Nav::NextSkip => Nav::NextSkipExtras,
        nav => nav,
    })
}

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
        let is_inside_node = matches!(
            first_nav,
            Some(Nav::Down | Nav::DownSkip | Nav::DownSkipExtras | Nav::DownExact)
        );

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
        let mut nav_modes = compute_nav_modes(items, is_inside_node, self.ctx.symbol_table);

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
        // Use the first *expression* index from `nav_modes`, not `items.first()`, so a
        // leading anchor (e.g. `{. (a)? ...}`) doesn't hide a skippable first item.
        let first_is_skippable = nav_modes
            .first()
            .and_then(|(idx, _)| items[*idx].as_expr())
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
        // Split capture effects: pre goes to FIRST item, post goes to LAST item
        let mut current_exit = exit;
        let count = nav_modes.len();
        let mut following_nav: Option<Nav> = None;
        for (i, (expr_idx, nav_override)) in nav_modes.into_iter().rev().enumerate() {
            let expr = items[expr_idx]
                .as_expr()
                .expect("nav_modes only contains expr indices");

            let is_last_expr = i == 0; // First in reversed loop = last in sequence
            let is_first_expr = i == count - 1; // Last in reversed loop = first in sequence

            let item_capture = CaptureEffects {
                pre: if is_first_expr {
                    capture.pre.clone()
                } else {
                    vec![]
                },
                post: if is_last_expr {
                    capture.post.clone()
                } else {
                    vec![]
                },
            };

            // An anchored follower checks adjacency at its exact position, so this
            // item must own a resumable search: the in-instruction candidate search
            // commits to its first match without a checkpoint and could never retry
            // at a later sibling when the anchored follower fails. Wrap it in a
            // position search unless it already owns its iteration (a quantifier).
            let followed_by_anchor = matches!(
                following_nav,
                Some(Nav::NextSkip | Nav::NextSkipExtras | Nav::NextExact)
            );
            let search_nav = resumable_search_nav(nav_override)
                .filter(|_| followed_by_anchor && !expr_owns_iteration(expr));

            current_exit = if let Some(nav) = search_nav {
                let body =
                    self.compile_expr_inner(expr, current_exit, Some(Nav::StayExact), item_capture);
                self.emit_position_search(nav, body)
            } else {
                self.compile_expr_inner(expr, current_exit, nav_override, item_capture)
            };
            following_nav = nav_override;
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
        let first_expr_idx = nav_modes[0].0;
        let first_expr = items[first_expr_idx]
            .as_expr()
            .expect("first item must be expression");

        // Compile the continuation with both navigations, or use exit if there is none.
        // When caller_skip_exit is provided and there is no follower, use it for the skip
        // path (this allows skip to bypass the Up instruction in the parent node).
        //
        // The two paths must slice `items` differently so that any anchor between the
        // optional first item and its follower survives into `compute_nav_modes`:
        //
        // - Skip path (first item absent): the anchor degrades to a leading anchor
        //   relative to the parent. Slicing *after* the first expression keeps the
        //   anchor, so it is re-derived as first-child (`Down*`) navigation.
        // - Match path (first item present): the follower is the first item's sibling.
        //   Slice *from* the follower (dropping the now-consumed leading anchor) and
        //   reuse the sibling navigation `compute_nav_modes` already derived for it,
        //   which carries the anchor's adjacency (`Next*`).
        let (skip_exit, match_exit) = if nav_modes.len() < 2 {
            (caller_skip_exit.unwrap_or(exit), exit)
        } else {
            let skip_rest = &items[first_expr_idx + 1..];
            let skip = self.compile_seq_items_inner(
                skip_rest,
                exit,
                is_inside_node,
                first_nav, // Down variant; overridden when a leading anchor is present
                capture.clone(),
                caller_skip_exit, // Propagate for nested skippables
            );

            let follower_idx = nav_modes[1].0;
            let match_rest = &items[follower_idx..];
            let mtch = self.compile_seq_items_inner(
                match_rest,
                exit,
                is_inside_node,
                nav_modes[1].1, // Follower's sibling nav (anchor-aware) for match path
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
            .ctx
            .type_ctx
            .get_term_info(&alt_expr)
            .and_then(|info| info.flow.type_id());
        let alt_type_shape = alt_type_id.and_then(|id| self.ctx.type_ctx.get_type(id));

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
        let merged_fields = alt_type_id.and_then(|id| self.ctx.type_ctx.get_struct_fields(id));

        // Branches match at the current candidate position. For resumable search
        // navs (`Down`, `Next`, `Stay`), the alternation itself emits the retry
        // wrapper below; otherwise the branch performs the exact navigation.
        let search_nav = resumable_search_nav(first_nav);
        let classifier = AnonymousClassifier::new(self.ctx.symbol_table);

        // Compile each branch, collecting entry labels
        let mut successors = Vec::new();
        for branch in branches.iter() {
            let Some(body) = branch.body() else {
                continue;
            };

            if is_enum {
                let branch_nav = nav_for_alt_branch(first_nav, search_nav, &body, &classifier);

                // Look up variant info by branch label (using BTreeMap order, not AST order)
                let label = branch.label().expect("tagged branch must have label");
                let label_text = label.text();
                let (variant_idx, payload_type_id) = variant_info
                    .iter()
                    .find(|(sym, _)| self.ctx.interner.resolve(**sym) == label_text)
                    .map(|(_, &(idx, type_id))| (idx, type_id))
                    .expect("variant must exist for labeled branch");

                // Create Enum effect for branch entry
                let e_effect = if let Some(type_id) = alt_type_id {
                    EffectIR::with_member(
                        EffectOpcode::Enum,
                        MemberRef::deferred_by_index(type_id, variant_idx),
                    )
                } else {
                    EffectIR::start_enum()
                };

                // Build capture effects: nest Enum/EndEnum inside outer effects
                let branch_capture = capture.clone().nest_scope(e_effect, EffectIR::end_enum());

                // Compile body with merged effects - no separate epsilon wrappers needed
                let body_entry = self.with_scope(payload_type_id, |this| {
                    this.compile_expr_inner(&body, exit, branch_nav, branch_capture)
                });

                successors.push(body_entry);
            } else {
                // Untagged branch: compile body with null injection for missing captures
                let null_effects: Vec<_> =
                    if let (Some(fields), Some(alt_type)) = (merged_fields, alt_type_id) {
                        let branch_captures = Self::collect_captures(&body);
                        fields
                            .iter()
                            .enumerate()
                            .filter(|(_, (sym, _))| {
                                !branch_captures.contains(self.ctx.interner.resolve(**sym))
                            })
                            .flat_map(|(idx, _)| {
                                [
                                    EffectIR::null(),
                                    EffectIR::with_member(
                                        EffectOpcode::Set,
                                        MemberRef::deferred_by_index(alt_type, idx as u16),
                                    ),
                                ]
                            })
                            .collect()
                    } else {
                        vec![]
                    };

                // Merge null injection with outer capture effects
                let branch_capture = capture.clone().with_pre_values(null_effects);

                let branch_nav = nav_for_alt_branch(first_nav, search_nav, &body, &classifier);
                let branch_entry = self.compile_expr_inner(&body, exit, branch_nav, branch_capture);
                successors.push(branch_entry);
            }
        }

        if successors.is_empty() {
            return exit;
        }

        let alt_entry = if successors.len() == 1 {
            successors[0]
        } else {
            // Emit epsilon branch to choose among alternatives.
            let entry = self.fresh_label();
            self.emit_epsilon(entry, successors);
            entry
        };

        if let Some(nav) = search_nav {
            return self.emit_position_search(nav, alt_entry);
        }

        alt_entry
    }
}
