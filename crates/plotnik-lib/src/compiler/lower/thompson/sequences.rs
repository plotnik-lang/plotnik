//! Sequence compilation.
//!
//! Handles compilation of:
//! - Sequences: `{a b c}` - siblings matched in order

use crate::bytecode::Nav;
use crate::compiler::lower::ir::{EffectIR, Label};
use crate::compiler::parse::ast::{self, SeqItem};

use super::NfaBuilder;
use super::capture::{CaptureEffects, PatternCtx, first_unmatched_close};
use super::navigation::{is_down_nav, resumable_search_nav};
use super::scope::SkipExit;

/// The sibling nav implied by a sequence's trailing anchor, used to mark the
/// last pattern as anchor-followed.
///
/// A trailing anchor (`{… .}`) constrains the last-child boundary via the parent
/// node's `Up*` nav. That check can still fail — the matched child is not the
/// last one — and must then retry at a later sibling. Treating the last item as
/// "followed by an anchor" routes its child search through the resumable
/// `emit_position_search` wrapper (when its own nav is a resumable search), so
/// the retry can happen. Returns `None` when the sequence has no trailing anchor.
fn trailing_anchor_follow_nav(items: &[SeqItem]) -> Option<Nav> {
    match items.last()? {
        SeqItem::Anchor(a) if a.is_exact() => Some(Nav::NextExact),
        SeqItem::Anchor(_) => Some(Nav::NextSkip),
        SeqItem::Pattern(_) => None,
    }
}

struct SequencePostEffects {
    item_post: Vec<EffectIR>,
    exit_post: Vec<EffectIR>,
}

fn split_sequence_tail_effects(post: Vec<EffectIR>) -> SequencePostEffects {
    let mut item_post = post;
    let exit_post = match first_unmatched_close(&item_post) {
        Some(split) => item_post.split_off(split),
        None => vec![],
    };

    SequencePostEffects {
        item_post,
        exit_post,
    }
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
    pub(super) skip_exit: Option<SkipExit>,
}

impl NfaBuilder<'_> {
    pub(super) fn compile_seq(&mut self, seq: &ast::SeqPattern, ctx: PatternCtx) -> Label {
        let PatternCtx {
            exit,
            nav: first_nav,
            capture,
            observe_value: _,
        } = ctx;
        let items: Vec<_> = seq.items().collect();
        if items.is_empty() {
            return exit;
        }

        let is_inside_node = matches!(
            first_nav,
            Some(Nav::Down | Nav::DownSkip | Nav::DownSkipExtras | Nav::DownExact)
        );

        self.compile_seq_items(SeqItemsCtx {
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
    pub(super) fn compile_seq_items(&mut self, ctx: SeqItemsCtx<'_>) -> Label {
        let SeqItemsCtx {
            items,
            exit,
            is_inside_node,
            first_nav,
            capture,
            skip_exit,
        } = ctx;

        let mut nav_modes = self
            .anchor_semantics
            .compute_nav_modes(items, is_inside_node);

        if nav_modes.is_empty() {
            return exit;
        }

        let first_pattern_nav = if let Some((_, first_mode)) = nav_modes.first_mut()
            && first_mode.is_none()
        {
            let nav = first_nav.or_else(|| is_inside_node.then_some(Nav::Down));
            *first_mode = nav;
            nav
        } else {
            nav_modes.first().and_then(|(_, n)| *n)
        };

        // Check if first pattern is skippable and uses Down navigation.
        // In this case, the skip path needs different navigation than the match path.
        // Also trigger when skip_exit is provided (for bypassing Up in parent node).
        // Use the first *pattern* index from `nav_modes`, not `items.first()`, so a
        // leading anchor (e.g. `{. (a)? ...}`) doesn't hide a skippable first item.
        let first_is_skippable = nav_modes
            .first()
            .and_then(|(idx, _)| items[*idx].as_pattern())
            .is_some_and(|p| self.is_skippable_item(p));
        // The skip path makes the follower the new "first" item, so it must re-derive
        // first-position navigation rather than the sibling `Next` the match path uses.
        // This is required whenever the first item navigates to a position (`Down*` into
        // a child, `Stay*` at an alternative's search candidate) instead of
        // advancing to a sibling — otherwise the skip path over-advances and never binds.
        let first_positions = is_down_nav(first_pattern_nav)
            || matches!(first_pattern_nav, Some(Nav::Stay | Nav::StayExact));
        // A caller-threaded skip exit must be honored at any nav: the all-skip
        // path has to reach it (a childless-node bypass, or a `Fail` prune),
        // not fall through to the match exit.
        let needs_skip_exit =
            first_is_skippable && (skip_exit.is_some() || (first_positions && nav_modes.len() > 1));

        if needs_skip_exit {
            return self.compile_seq_items_with_skip_exit(
                &nav_modes,
                SeqItemsCtx {
                    items,
                    exit,
                    is_inside_node,
                    first_nav: first_pattern_nav,
                    capture,
                    skip_exit,
                },
            );
        }

        // Scope open/close effects merge onto the boundary items to avoid extra epsilons.
        // That merge is unsound when the boundary item is skippable: its skip path drops
        // the merged effect, unbalancing the path (e.g. VariantClose without VariantOpen). So a
        // skippable boundary's *scope* effects move to a dominating epsilon every path
        // crosses. Value effects (`Node`/`RecordSet`/…) stay on the matched item — they need its
        // cursor position and its skip-path null injection — so only scope effects move.
        let last_is_skippable = nav_modes
            .last()
            .and_then(|(idx, _)| items[*idx].as_pattern())
            .is_some_and(|p| self.is_skippable_item(p));

        // Build chain in reverse: last pattern exits to `exit`, each prior exits to next.
        let mut current_exit = exit;
        let last_post = if last_is_skippable {
            let tail_effects = split_sequence_tail_effects(capture.post);
            if !tail_effects.exit_post.is_empty() {
                current_exit = self.emit_effects_epsilon(
                    current_exit,
                    tail_effects.exit_post,
                    CaptureEffects::default(),
                );
            }
            tail_effects.item_post
        } else {
            capture.post
        };
        let count = nav_modes.len();
        // Seed the reverse walk so the last pattern sees a trailing anchor as
        // its follower: its child search then stays resumable for the up-nav
        // lastness retry. Interior items overwrite this with their real follower.
        let mut following_nav: Option<Nav> = trailing_anchor_follow_nav(items);
        for (i, (pattern_idx, nav_override)) in nav_modes.into_iter().rev().enumerate() {
            let pattern = items[pattern_idx]
                .as_pattern()
                .expect("nav_modes only contains pattern indices");

            let is_last_pattern = i == 0; // First in reversed loop = last in sequence
            let is_first_pattern = i == count - 1; // Last in reversed loop = first in sequence

            let item_capture = CaptureEffects {
                pre: if is_first_pattern && !first_is_skippable {
                    capture.pre.clone()
                } else {
                    vec![]
                },
                post: if is_last_pattern {
                    last_post.clone()
                } else {
                    vec![]
                },
            };

            // An anchored follower checks the gap at its exact position, so this
            // item must own a resumable search: the in-instruction candidate search
            // commits to its first match without a checkpoint and could never retry
            // at a later sibling when the anchored follower fails. Wrap it in a
            // position search unless it already owns its iteration (a quantifier).
            let followed_by_anchor = matches!(
                following_nav,
                Some(Nav::NextSkip | Nav::NextSkipExtras | Nav::NextExact)
            );
            let search_nav = resumable_search_nav(nav_override)
                .filter(|_| followed_by_anchor && !self.item_owns_iteration(pattern));

            current_exit = if let Some(nav) = search_nav {
                let pattern_ctx = PatternCtx {
                    exit: current_exit,
                    nav: Some(Nav::StayExact),
                    capture: item_capture,
                    observe_value: false,
                };
                let body = self.dispatch_pattern(pattern, pattern_ctx);
                self.emit_position_search(nav, body)
            } else {
                let pattern_ctx = PatternCtx {
                    exit: current_exit,
                    nav: nav_override,
                    capture: item_capture,
                    observe_value: false,
                };
                self.dispatch_pattern(pattern, pattern_ctx)
            };
            following_nav = nav_override;
        }
        if first_is_skippable {
            current_exit = self.wrap_entry_pre(current_exit, capture.pre);
        }
        current_exit
    }

    /// Compile sequence items where the first item is skippable and navigates to a
    /// position (`Down*` into a child, or `Stay*` at an alternative's candidate).
    ///
    /// When the first item (`?`/`*`) is skipped, the next item becomes the "first"
    /// and must re-derive first-position navigation instead of the sibling `Next` the
    /// match path uses. This requires compiling the continuation twice.
    ///
    /// Sequence-level scope effects (`capture`: e.g. `VariantOpen`/`VariantClose` for a variant
    /// value) wrap the whole body on single dominating epsilons — open before the
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
        let first_pattern_idx = nav_modes[0].0;
        let first_pattern = items[first_pattern_idx]
            .as_pattern()
            .expect("first item must be pattern");

        // Close the *scope* on a single exit epsilon every continuation converges to.
        // From the first scope close onward the suffix runs on every path; the value
        // prefix (`post_keep`) instead rides the sequence's last matched item — it needs
        // that item's cursor position and skip null injection, so an epsilon would capture
        // the wrong node or miss null on the skip path. Split positionally so a
        // close and its consumer (e.g. `[VariantClose, ArrayPush]`) stay together and in order.
        let tail_effects = split_sequence_tail_effects(capture.post);
        let exit_post = tail_effects.exit_post;
        let exit = self.emit_effects_if_nonempty(exit, exit_post.clone());
        let caller_skip_exit = match caller_skip_exit {
            Some(SkipExit::To(skip)) => {
                Some(SkipExit::To(self.emit_effects_if_nonempty(skip, exit_post)))
            }
            other => other,
        };

        // Compile the continuation with both navigations, or use exit if there is none.
        // When caller_skip_exit is provided and there is no follower, use it for the skip
        // path (this allows skip to bypass the Up instruction in the parent node).
        //
        // The two paths must slice `items` differently so that any anchor between the
        // first `?`/`*` item and its follower survives into `compute_nav_modes`:
        //
        // - Skip path (first item absent): the anchor degrades to a leading anchor
        //   relative to the parent. Slicing *after* the first pattern keeps the
        //   anchor, so it is re-derived as first-child (`Down*`) navigation.
        // - Match path (first item present): the follower is the first item's sibling.
        //   Slice *from* the follower (dropping the now-consumed leading anchor) and
        //   reuse the sibling navigation `compute_nav_modes` already derived for it,
        //   which carries the anchor's gap policy (`Next*`).
        let (skip_exit, match_exit, first_post) = if nav_modes.len() < 2 {
            // The skippable item is the only (and last) item, so it carries `post_keep`.
            (
                caller_skip_exit.unwrap_or(SkipExit::To(exit)),
                exit,
                CaptureEffects::new_post(tail_effects.item_post),
            )
        } else {
            // The follower is the last item; `post_keep` rides its continuation.
            let cont = CaptureEffects::new_post(tail_effects.item_post);
            let skip_rest = &items[first_pattern_idx + 1..];
            let skip = self.compile_seq_items(SeqItemsCtx {
                items: skip_rest,
                exit,
                is_inside_node,
                first_nav, // Position variant; overridden when a leading anchor is present
                capture: cont.clone(),
                skip_exit: caller_skip_exit, // Propagate for nested skippables
            });

            let follower_idx = nav_modes[1].0;
            let match_rest = &items[follower_idx..];
            let mtch = self.compile_seq_items(SeqItemsCtx {
                items: match_rest,
                exit,
                is_inside_node,
                first_nav: nav_modes[1].1, // Follower's sibling nav (anchor-aware) for match path
                capture: cont,
                skip_exit: None, // Match path doesn't need skip exit
            });
            (SkipExit::To(skip), mtch, CaptureEffects::default())
        };

        let pattern_ctx = PatternCtx {
            exit: match_exit,
            nav: first_nav,
            capture: first_post,
            observe_value: false,
        };
        let entry = self.compile_nullable_pattern(first_pattern, pattern_ctx, skip_exit);

        // Open the scope on a single entry epsilon every path crosses first.
        self.wrap_entry_pre(entry, capture.pre)
    }
}
