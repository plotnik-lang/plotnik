//! Sequence and alternation compilation.
//!
//! Handles compilation of:
//! - Sequences: `{a b c}` - siblings matched in order
//! - Alternations: `[a b c]` - first matching branch wins

use std::collections::{BTreeMap, HashSet};

use plotnik_core::Symbol;

use crate::analyze::type_check::{TypeId, TypeShape};
use crate::bytecode::{EffectIR, InstructionIR, Label, MemberRef, NodeTypeIR};
use crate::parser::ast::{self, Expr, SeqItem};
use plotnik_bytecode::{EffectOpcode, Nav};

use super::Compiler;
use super::capture::{CaptureEffects, ExprCtx};
use super::navigation::{
    AnonymousClassifier, compute_nav_modes, expr_owns_iteration, is_down_nav,
    is_skippable_quantifier, resumable_search_nav,
};
use super::scope::SplitExits;

/// The sibling nav implied by a sequence's trailing anchor, used to mark the
/// last expression as anchor-followed.
///
/// A trailing anchor (`{… .}`) enforces last-child adjacency via the parent
/// node's `Up*` nav. That check can still fail — the matched child is not the
/// last one — and must then retry at a later sibling. Treating the last item as
/// "followed by an anchor" routes its child search through the resumable
/// `emit_position_search` wrapper (when its own nav is a resumable search), so
/// the retry can happen. Returns `None` when the sequence has no trailing anchor.
fn trailing_anchor_follow_nav(items: &[SeqItem]) -> Option<Nav> {
    match items.last()? {
        SeqItem::Anchor(a) if a.is_strict() => Some(Nav::NextExact),
        SeqItem::Anchor(_) => Some(Nav::NextSkip),
        SeqItem::Expr(_) => None,
    }
}

/// The alternation's resumable search nav (from [`resumable_search_nav`]), kept
/// distinct from a branch's `first_nav` so the two adjacent `Option<Nav>` inputs
/// to [`nav_for_alt_branch`] cannot be transposed. `Some` means the alternation
/// owns the retry wrapper and each branch matches exactly at the candidate
/// (`StayExact`).
#[derive(Clone, Copy)]
struct SearchNav(Option<Nav>);

fn exact_nav_for_alt_branch(first_nav: Option<Nav>, search_nav: SearchNav) -> Option<Nav> {
    if search_nav.0.is_some() {
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
    search_nav: SearchNav,
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

/// A scope-closing effect (`EndArr`/`EndObj`/`EndEnum`/`SuppressEnd`).
///
/// Used to split a sequence's post-effects when its last item is skippable. The list
/// is `[value effects…, scope close, consumers of the scope value…]` — e.g.
/// `[Node, Set]`, `[EndEnum]`, or `[EndEnum, Push]`. From the *first* scope close
/// onward, the effects belong to the whole sequence (they close its scope and consume
/// the produced value) and must run on every path, so that contiguous suffix rides a
/// dominating epsilon. The prefix before it is the item's own value capture — it needs
/// the item's `matched_node` and skip-path null injection, so it stays on the item.
/// Splitting positionally (not by effect kind) keeps a close and its consumer together
/// and in order.
fn is_scope_close_effect(e: &EffectIR) -> bool {
    matches!(
        e.opcode(),
        EffectOpcode::EndArr
            | EffectOpcode::EndObj
            | EffectOpcode::EndEnum
            | EffectOpcode::SuppressEnd
    )
}

/// Parameters threaded through sequence-item compilation.
///
/// `skip_exit`, when present, redirects the skip path of a skippable first item
/// past the parent node's `Up` instruction (childless-node bypass).
pub(super) struct SeqItemsCtx<'a> {
    pub(super) items: &'a [SeqItem],
    pub(super) exit: Label,
    pub(super) is_inside_node: bool,
    pub(super) first_nav: Option<Nav>,
    pub(super) capture: CaptureEffects,
    pub(super) skip_exit: Option<Label>,
}

impl Compiler<'_> {
    pub(super) fn compile_seq_inner(&mut self, seq: &ast::SeqExpr, ctx: ExprCtx) -> Label {
        let ExprCtx {
            exit,
            nav: first_nav,
            capture,
        } = ctx;
        let items: Vec<_> = seq.items().collect();
        if items.is_empty() {
            return exit;
        }

        let is_inside_node = matches!(
            first_nav,
            Some(Nav::Down | Nav::DownSkip | Nav::DownSkipExtras | Nav::DownExact)
        );

        self.compile_seq_items_inner(SeqItemsCtx {
            items: &items,
            exit,
            is_inside_node,
            first_nav,
            capture,
            skip_exit: None,
        })
    }

    /// Compile sequence items with capture effects (passed to last item).
    ///
    /// When `skip_exit` is provided, the skip path of the first skippable item
    /// will use this exit instead of `exit`. This is used when inside a node
    /// where skip paths should bypass the Up instruction.
    pub(super) fn compile_seq_items_inner(&mut self, ctx: SeqItemsCtx<'_>) -> Label {
        let SeqItemsCtx {
            items,
            exit,
            is_inside_node,
            first_nav,
            capture,
            skip_exit,
        } = ctx;

        let mut nav_modes = compute_nav_modes(items, is_inside_node, self.ctx.symbol_table);

        if nav_modes.is_empty() {
            return exit;
        }

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
        // The skip path makes the follower the new "first" item, so it must re-derive
        // first-position navigation rather than the sibling `Next` the match path uses.
        // This is required whenever the first item navigates to a position (`Down*` into
        // a child, `Stay*` at an alternation branch's search candidate) instead of
        // advancing to a sibling — otherwise the skip path over-advances and never binds.
        let first_positions = is_down_nav(first_expr_nav)
            || matches!(first_expr_nav, Some(Nav::Stay | Nav::StayExact));
        let needs_skip_exit =
            first_is_skippable && first_positions && (nav_modes.len() > 1 || skip_exit.is_some());

        if needs_skip_exit {
            return self.compile_seq_items_with_skip_exit(
                &nav_modes,
                SeqItemsCtx {
                    items,
                    exit,
                    is_inside_node,
                    first_nav: first_expr_nav,
                    capture,
                    skip_exit,
                },
            );
        }

        // Scope open/close effects merge onto the boundary items to avoid extra epsilons.
        // That merge is unsound when the boundary item is skippable: its skip path drops
        // the merged effect, unbalancing the path (e.g. EndEnum with no Enum). So a
        // skippable boundary's *scope* effects move to a dominating epsilon every path
        // crosses. Value effects (Node/Set/…) stay on the matched item — they need its
        // matched_node and its skip-path null injection — so only scope effects move.
        let last_is_skippable = nav_modes
            .last()
            .and_then(|(idx, _)| items[*idx].as_expr())
            .is_some_and(is_skippable_quantifier);

        // Build chain in reverse: last expression exits to `exit`, each prior exits to next.
        let mut current_exit = exit;
        let last_post = match last_is_skippable
            .then(|| capture.post.iter().position(is_scope_close_effect))
            .flatten()
        {
            Some(split) => {
                current_exit = self.emit_effects_epsilon(
                    current_exit,
                    capture.post[split..].to_vec(),
                    CaptureEffects::default(),
                );
                capture.post[..split].to_vec()
            }
            None => capture.post.clone(),
        };
        let count = nav_modes.len();
        // Seed the reverse walk so the last expression sees a trailing anchor as
        // its follower: its child search then stays resumable for the up-nav
        // lastness retry. Interior items overwrite this with their real follower.
        let mut following_nav: Option<Nav> = trailing_anchor_follow_nav(items);
        for (i, (expr_idx, nav_override)) in nav_modes.into_iter().rev().enumerate() {
            let expr = items[expr_idx]
                .as_expr()
                .expect("nav_modes only contains expr indices");

            let is_last_expr = i == 0; // First in reversed loop = last in sequence
            let is_first_expr = i == count - 1; // Last in reversed loop = first in sequence

            let item_capture = CaptureEffects {
                pre: if is_first_expr && !first_is_skippable {
                    capture.pre.clone()
                } else {
                    vec![]
                },
                post: if is_last_expr {
                    last_post.clone()
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
                let body = self.compile_expr_inner(
                    expr,
                    ExprCtx {
                        exit: current_exit,
                        nav: Some(Nav::StayExact),
                        capture: item_capture,
                    },
                );
                self.emit_position_search(nav, body)
            } else {
                self.compile_expr_inner(
                    expr,
                    ExprCtx {
                        exit: current_exit,
                        nav: nav_override,
                        capture: item_capture,
                    },
                )
            };
            following_nav = nav_override;
        }
        if first_is_skippable {
            current_exit = self.wrap_entry_pre(current_exit, capture.pre.clone());
        }
        current_exit
    }

    /// Compile sequence items where the first item is skippable and navigates to a
    /// position (`Down*` into a child, or `Stay*` at an alternation branch's candidate).
    ///
    /// When the first item (optional/star) is skipped, the next item becomes the "first"
    /// and must re-derive first-position navigation instead of the sibling `Next` the
    /// match path uses. This requires compiling the continuation twice.
    ///
    /// Sequence-level scope effects (`capture`: e.g. `Enum`/`EndEnum` for a tagged
    /// variant) wrap the whole body on single dominating epsilons — open before the
    /// skippable item, close after the continuation — so they execute exactly once on
    /// both the skip and match paths. Merging them onto items instead would drop the
    /// open on the skip path (skippable first item) and leave the path unbalanced.
    ///
    /// When `ctx.skip_exit` is provided and there are no remaining items, the skip
    /// path uses this exit (to bypass Up in parent node) while match path uses `exit`.
    fn compile_seq_items_with_skip_exit(
        &mut self,
        nav_modes: &[(usize, Option<Nav>)],
        ctx: SeqItemsCtx<'_>,
    ) -> Label {
        let SeqItemsCtx {
            items,
            exit,
            is_inside_node,
            first_nav,
            capture,
            skip_exit: caller_skip_exit,
        } = ctx;
        let first_expr_idx = nav_modes[0].0;
        let first_expr = items[first_expr_idx]
            .as_expr()
            .expect("first item must be expression");

        // Close the *scope* on a single exit epsilon every continuation converges to.
        // From the first scope close onward the suffix runs on every path; the value
        // prefix (`post_keep`) instead rides the sequence's last matched item — it needs
        // matched_node and the item's skip null injection, so an epsilon would capture
        // the wrong node or miss matched_node on the skip path. Split positionally so a
        // close and its consumer (e.g. `[EndEnum, Push]`) stay together and in order.
        let (post_keep, post_close): (Vec<_>, Vec<_>) =
            match capture.post.iter().position(is_scope_close_effect) {
                Some(split) => (
                    capture.post[..split].to_vec(),
                    capture.post[split..].to_vec(),
                ),
                None => (capture.post.clone(), vec![]),
            };
        let exit = if post_close.is_empty() {
            exit
        } else {
            self.emit_effects_epsilon(exit, post_close, CaptureEffects::default())
        };

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
        let (skip_exit, match_exit, first_post) = if nav_modes.len() < 2 {
            // The skippable item is the only (and last) item, so it carries `post_keep`.
            (
                caller_skip_exit.unwrap_or(exit),
                exit,
                CaptureEffects::new_post(post_keep),
            )
        } else {
            // The follower is the last item; `post_keep` rides its continuation.
            let cont = CaptureEffects::new_post(post_keep);
            let skip_rest = &items[first_expr_idx + 1..];
            let skip = self.compile_seq_items_inner(SeqItemsCtx {
                items: skip_rest,
                exit,
                is_inside_node,
                first_nav, // Position variant; overridden when a leading anchor is present
                capture: cont.clone(),
                skip_exit: caller_skip_exit, // Propagate for nested skippables
            });

            let follower_idx = nav_modes[1].0;
            let match_rest = &items[follower_idx..];
            let mtch = self.compile_seq_items_inner(SeqItemsCtx {
                items: match_rest,
                exit,
                is_inside_node,
                first_nav: nav_modes[1].1, // Follower's sibling nav (anchor-aware) for match path
                capture: cont,
                skip_exit: None, // Match path doesn't need skip exit
            });
            (skip, mtch, CaptureEffects::default())
        };

        let entry = self.compile_skippable_with_exits(
            first_expr,
            SplitExits {
                match_exit,
                skip_exit,
            },
            first_nav,
            first_post,
        );

        // Open the scope on a single entry epsilon every path crosses first.
        self.wrap_entry_pre(entry, capture.pre.clone())
    }

    /// When an alternation sits immediately before a soft sibling anchor into a
    /// *named* follower (`[(b) ","] . (a)`), the follower's entry was classified
    /// conservatively as `NextSkipExtras` because *some* branch may match an
    /// anonymous node (`compute_nav_modes` sees the whole alternation, not the
    /// branch that actually matched). A named branch on the matched side deserves
    /// the both-sides-named soft skip (`NextSkip`), which also skips anonymous
    /// siblings. Emit a twin of the follower's entry instruction with `NextSkip`,
    /// sharing its successors and effects, and return its label so named branches
    /// route to it. Exactly one of the two entries runs per match path, so cloning
    /// the effects is sound.
    ///
    /// Returns `None` — caller stays conservative — unless the follower's entry is
    /// a single `Match` carrying `NextSkipExtras` on a `Named` node, the one shape
    /// where the upgrade is both safe and needed. Anonymous/`_` followers are
    /// intrinsically extras-only; refs (`Call`) and scope-wrapped followers
    /// (epsilon entry) carry no such instruction and are left for follow-up.
    fn clone_named_follower_skip_entry(&mut self, exit: Label) -> Option<Label> {
        let mut twin = {
            let InstructionIR::Match(m) = self.instructions.iter().find(|i| i.label() == exit)?
            else {
                return None;
            };
            if m.nav != Nav::NextSkipExtras || !matches!(m.node_type, NodeTypeIR::Named(_)) {
                return None;
            }
            m.clone()
        };

        twin.label = self.fresh_label();
        twin.nav = Nav::NextSkip;
        let label = twin.label;
        self.instructions.push(twin.into());
        Some(label)
    }

    /// Compile an alternation with capture effects (passed to each branch).
    pub(super) fn compile_alt_inner(&mut self, alt: &ast::AltExpr, ctx: ExprCtx) -> Label {
        let ExprCtx {
            exit,
            nav: first_nav,
            capture,
        } = ctx;
        let branches: Vec<_> = alt.branches().collect();
        if branches.is_empty() {
            return exit;
        }

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

        // BTreeMap order gives stable variant indices independent of AST iteration order.
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
        let branch_search = SearchNav(search_nav);
        let classifier = AnonymousClassifier::new(self.ctx.symbol_table);

        // A branch is "named" (eligible for the soft-skip follower twin) when it
        // cannot match an anonymous node and does not own its own iteration. A
        // quantified branch's zero-match path leaves no named node on the anchor's
        // left, so the soft-skip upgrade is unsound there; quantified followers are
        // a separate (deferred) case. The anonymity test is whole-branch, matching
        // `nav_for_alt_branch`'s before-anchor classification: a sequence branch
        // with a named tail but interior punctuation stays conservative (safe, not
        // a wrong match) until a trailing-position classifier exists. Computed once
        // and reused for both the twin gate and per-branch routing.
        let branch_named: Vec<bool> = branches
            .iter()
            .map(|b| {
                b.body().is_some_and(|body| {
                    !expr_owns_iteration(&body) && !classifier.expr_may_match_anonymous(Some(&body))
                })
            })
            .collect();

        // A soft sibling anchor after this alternation classified the named follower
        // conservatively (`NextSkipExtras`) because some branch may match an anonymous
        // node. Give named branches a `NextSkip` twin of that follower to route to.
        // Only worth cloning when at least one branch is itself named.
        let named_exit = branch_named
            .iter()
            .any(|&named| named)
            .then(|| self.clone_named_follower_skip_entry(exit))
            .flatten();

        let mut successors = Vec::new();
        for (branch_idx, branch) in branches.iter().enumerate() {
            let Some(body) = branch.body() else {
                continue;
            };

            // Named branches route to the soft-skip follower twin; anonymous branches
            // keep the conservative extras-only follower at `exit`.
            let branch_exit = match named_exit {
                Some(skip) if branch_named[branch_idx] => skip,
                _ => exit,
            };

            if is_enum {
                let branch_nav = nav_for_alt_branch(first_nav, branch_search, &body, &classifier);

                // Look up variant info by branch label (using BTreeMap order, not AST order)
                let label = branch.label().expect("tagged branch must have label");
                let label_text = label.text();
                let (variant_idx, payload_type_id) = self
                    .ctx
                    .interner
                    .get(label_text)
                    .and_then(|sym| variant_info.get(&sym))
                    .map(|&(idx, type_id)| (idx, type_id))
                    .expect("variant must exist for labeled branch");

                let e_effect = if let Some(type_id) = alt_type_id {
                    EffectIR::with_member(EffectOpcode::Enum, MemberRef::new(type_id, variant_idx))
                } else {
                    EffectIR::start_enum()
                };

                let branch_capture = capture.clone().nest_scope(e_effect, EffectIR::end_enum());

                let body_entry = self.with_scope(payload_type_id, |this| {
                    this.compile_expr_inner(
                        &body,
                        ExprCtx {
                            exit: branch_exit,
                            nav: branch_nav,
                            capture: branch_capture,
                        },
                    )
                });

                successors.push(body_entry);
            } else {
                // Untagged branch: inject a default value for every merged field this
                // branch does not itself produce, so the output shape stays stable.
                // "Produces" means a top-level (bubbling) field — a capture nested in a
                // child scope (`{...} @row`) belongs to that scope, not here. The
                // branch's inferred bubble is the single source of truth; a syntactic
                // capture walk would miscount nested names and drop a needed default.
                let null_effects: Vec<EffectIR> = if let Some(fields) = merged_fields {
                    let provided: HashSet<Symbol> = self
                        .ctx
                        .type_ctx
                        .get_term_info(&body)
                        .and_then(|info| info.flow.type_id())
                        .and_then(|id| self.ctx.type_ctx.get_struct_fields(id))
                        .map(|f| f.keys().copied().collect())
                        .unwrap_or_default();
                    fields
                        .iter()
                        .filter(|(sym, _)| !provided.contains(*sym))
                        .flat_map(|(sym, field_info)| {
                            // Resolve the default into the enclosing scope — the same
                            // struct this branch's real captures Set into — so the member
                            // ref names a type an entrypoint result reaches. The
                            // alternation's own merged struct is otherwise unreachable;
                            // pointing defaults at it would force dead-type elimination to
                            // keep a parallel root set of effect-referenced types alive.
                            let name = self.ctx.interner.resolve(*sym);
                            let member_ref = self.lookup_member_in_scope(name).expect(
                                "alternation bubbling field must resolve in enclosing scope",
                            );
                            let set = EffectIR::with_member(EffectOpcode::Set, member_ref);
                            // A non-optional list defaults to `[]`; everything else
                            // — scalars, and optional lists like `((x)+ @a)?` —
                            // defaults to null. The `optional` flag, not the array
                            // shape, is the source of truth, matching the relaxed
                            // type from `relax_for_absence`.
                            let is_required_list = !field_info.optional
                                && matches!(
                                    self.ctx.type_ctx.get_type(field_info.type_id),
                                    Some(TypeShape::Array { .. })
                                );
                            if is_required_list {
                                vec![EffectIR::start_arr(), EffectIR::end_arr(), set]
                            } else {
                                vec![EffectIR::null(), set]
                            }
                        })
                        .collect()
                } else {
                    vec![]
                };

                let branch_capture = capture.clone().with_pre_values(null_effects);

                let branch_nav = nav_for_alt_branch(first_nav, branch_search, &body, &classifier);
                let branch_entry = self.compile_expr_inner(
                    &body,
                    ExprCtx {
                        exit: branch_exit,
                        nav: branch_nav,
                        capture: branch_capture,
                    },
                );
                successors.push(branch_entry);
            }
        }

        if successors.is_empty() {
            return exit;
        }

        let alt_entry = if successors.len() == 1 {
            successors[0]
        } else {
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
