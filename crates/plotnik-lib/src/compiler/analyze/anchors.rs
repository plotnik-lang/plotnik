//! Shared anchor truth: the one place sibling-gap semantics are computed.
//!
//! Both codegen (`lower/thompson`) and the grammar satisfiability checker
//! (`analyze/grammar/satisfiability`) must agree on exactly what an anchor lets a gap
//! skip — if they drift, the checker rejects what the VM would have matched (or
//! the reverse). So the nav computation lives here, in `analyze`, where both may
//! depend on it, and codegen re-exports it rather than forking it. [`GapClass`]
//! projects those navs onto the skip classes the checker reasons over; its
//! `admits` method delegates to the same core skip class the VM uses.

use crate::bytecode::Nav;
use crate::compiler::analyze::shape::PatternFacts;
use crate::compiler::parse::ast::{Pattern, SeqItem};
use crate::core::{NodeClass, SkipClass};

/// What a gap between two query patterns may skip over, projected from the same
/// [`Nav`] codegen emits so the checker and the VM cannot drift.
///
/// The axes mirror the VM exactly (`vm/engine/cursor.rs`): a node is *anonymous*
/// when it is not named and *extra* when the parser marked it as an extra (a
/// comment, say). The two are independent — a named comment is `(false, true)`, an
/// anonymous brace `(true, false)`. A broad skip clears `anonymous || extra` (the
/// VM's `is_trivia`); a narrow skip clears only `extra`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GapClass {
    /// No anchor: any node may sit in the gap (the VM's `SkipPolicy::Any`).
    Any,
    /// Soft `.` with both operands named: skip anonymous tokens and extras.
    AnonymousAndExtras,
    /// Soft `.` with an anonymous operand: skip extras only.
    ExtrasOnly,
    /// Exact `.!`: no syntax-tree node may intervene.
    Exact,
}

impl GapClass {
    pub(crate) fn skip_class(self) -> SkipClass {
        match self {
            Self::Any => SkipClass::Any,
            Self::AnonymousAndExtras => SkipClass::Trivia,
            Self::ExtrasOnly => SkipClass::Extras,
            Self::Exact => SkipClass::Exact,
        }
    }

    /// Whether a node may be skipped across this gap.
    pub(crate) fn admits(self, node: NodeClass) -> bool {
        self.skip_class().admits(node)
    }

    /// Rank by permissiveness. The classes nest — `Exact ⊂ ExtrasOnly ⊂
    /// AnonymousAndExtras ⊂ Any` — so a total order captures their intersection and
    /// union exactly, which is what [`tighten`](Self::tighten)/[`loosen`](Self::loosen)
    /// need.
    fn permissiveness(self) -> u8 {
        match self {
            Self::Exact => 0,
            Self::ExtrasOnly => 1,
            Self::AnonymousAndExtras => 2,
            Self::Any => 3,
        }
    }

    /// The more restrictive of two gaps — their intersection. The skip permission of one
    /// path is bounded by the tightest gap on it.
    pub fn tighten(self, other: Self) -> Self {
        if self.permissiveness() <= other.permissiveness() {
            self
        } else {
            other
        }
    }

    /// The more permissive of two gaps — their union. A state reachable by several paths
    /// admits a skip if any path does.
    pub fn loosen(self, other: Self) -> Self {
        if self.permissiveness() >= other.permissiveness() {
            self
        } else {
            other
        }
    }

    /// Project a codegen [`Nav`] onto the gap it opens, or `None` for navs that
    /// drive no sibling gap (pure control flow, or a `Stay` that does not move).
    ///
    /// The skip suffix is what carries: a plain `Next`/`Down`/`Up` skips anything,
    /// a `*Skip`/`UpSkipTrivia` is a broad skip, a `*SkipExtras`/`UpSkipExtras` a
    /// narrow one, an `*Exact` skips nothing. `UpSkipTrivia` is the broad skip
    /// despite its name — it mirrors `is_trivia`, not `is_extra` (see `cursor.rs`).
    pub fn from_nav(nav: Nav) -> Option<Self> {
        let class = match nav {
            Nav::Next | Nav::Down | Nav::Up(_) => Self::Any,
            Nav::NextSkip | Nav::DownSkip | Nav::UpSkipTrivia(_) => Self::AnonymousAndExtras,
            Nav::NextSkipExtras | Nav::DownSkipExtras | Nav::UpSkipExtras(_) => Self::ExtrasOnly,
            Nav::NextExact | Nav::DownExact | Nav::UpExact(_) => Self::Exact,
            // Childless asserts at the current position without moving; it
            // opens no sibling gap, like the other stay-in-place navs.
            Nav::Epsilon
            | Nav::Stay
            | Nav::StayExact
            | Nav::ChildlessSkipTrivia
            | Nav::ChildlessSkipExtras
            | Nav::ChildlessExact => return None,
        };
        Some(class)
    }
}

/// Computes anchor-derived navigation from retained pattern facts.
pub struct AnchorSemantics<'a> {
    pattern_facts: &'a PatternFacts,
}

/// Whether this pattern's immediate alternatives compile alternative-local entry navs.
///
/// A soft anchor before such a pattern is decided by each alternative, not by the
/// alternation's whole-pattern anonymous classification.
pub(crate) fn has_direct_alternative_nav(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::Alternation(_) => true,
        Pattern::CapturedPattern(cap) => {
            cap.inner().as_ref().is_some_and(has_direct_alternative_nav)
        }
        _ => false,
    }
}

impl<'a> AnchorSemantics<'a> {
    pub fn new(pattern_facts: &'a PatternFacts) -> Self {
        Self { pattern_facts }
    }

    pub fn pattern_may_match_anonymous_node(&self, pattern: Option<&Pattern>) -> bool {
        pattern.is_some_and(|pattern| self.pattern_facts.pattern_may_match_anonymous_node(pattern))
    }

    /// Check for trailing anchor in items, descending into a sole-child sequence if needed.
    pub fn check_trailing_anchor(&self, items: &[SeqItem]) -> (bool, Option<Nav>) {
        if matches!(items.last(), Some(SeqItem::Anchor(_))) {
            let trailing_is_exact = items
                .iter()
                .rev()
                .take_while(|item| matches!(item, SeqItem::Anchor(_)))
                .any(|item| matches!(item, SeqItem::Anchor(anchor) if anchor.is_exact()));
            if trailing_is_exact {
                return (true, Some(Nav::UpExact(1)));
            }

            let prev_pattern = items.iter().rev().skip(1).find_map(|item| {
                if let SeqItem::Pattern(e) = item {
                    Some(e)
                } else {
                    None
                }
            });

            let nav = if self.pattern_may_match_anonymous_node(prev_pattern) {
                Nav::UpSkipExtras(1)
            } else {
                Nav::UpSkipTrivia(1)
            };
            return (true, Some(nav));
        }

        if items.len() == 1
            && let Some(SeqItem::Pattern(Pattern::SeqPattern(seq))) = items.first()
        {
            let seq_items: Vec<_> = seq.items().collect();
            return self.check_trailing_anchor(&seq_items);
        }

        (false, None)
    }

    /// The anchored entry nav a leading anchor imposes on the first pattern, or
    /// `None` when the item list has no leading anchor. Descends into a sole-child
    /// sequence like [`check_trailing_anchor`](Self::check_trailing_anchor), and
    /// reads the nav off [`compute_nav_modes`](Self::compute_nav_modes) so the
    /// empty-match arm of the anchor cannot drift from the arm that matches.
    pub fn check_leading_anchor(&self, items: &[SeqItem]) -> Option<Nav> {
        if items.len() == 1
            && let Some(SeqItem::Pattern(Pattern::SeqPattern(seq))) = items.first()
        {
            let seq_items: Vec<_> = seq.items().collect();
            return self.check_leading_anchor(&seq_items);
        }

        let is_inside_node = true;
        let (_, nav) = self
            .compute_nav_modes(items, is_inside_node)
            .into_iter()
            .next()?;
        match nav {
            Some(nav @ (Nav::DownExact | Nav::DownSkip | Nav::DownSkipExtras)) => Some(nav),
            _ => None,
        }
    }

    pub fn compute_nav_modes(
        &self,
        items: &[SeqItem],
        is_inside_node: bool,
    ) -> Vec<(usize, Option<Nav>)> {
        let mut result = Vec::new();
        let mut pending_anchor_exact = None;
        let mut prev_is_anonymous = false;
        let mut is_first_pattern = true;

        for (idx, item) in items.iter().enumerate() {
            match item {
                SeqItem::Anchor(anchor) => {
                    pending_anchor_exact =
                        Some(pending_anchor_exact.unwrap_or(false) || anchor.is_exact());
                }
                SeqItem::Pattern(pattern) => {
                    let current_is_anonymous =
                        self.pattern_facts.pattern_may_match_anonymous_node(pattern);
                    // Alternation alternatives compile their own entry nav, so the alternative body—not
                    // the whole alternation—decides whether soft anchors use extras-only nav.
                    let current_is_anonymous_for_anchor = if has_direct_alternative_nav(pattern) {
                        false
                    } else {
                        current_is_anonymous
                    };
                    let nav = if let Some(is_exact) = pending_anchor_exact {
                        if is_first_pattern && is_inside_node {
                            Some(if is_exact {
                                Nav::DownExact
                            } else if current_is_anonymous_for_anchor {
                                Nav::DownSkipExtras
                            } else {
                                Nav::DownSkip
                            })
                        } else if !is_first_pattern {
                            Some(if is_exact {
                                Nav::NextExact
                            } else if prev_is_anonymous || current_is_anonymous_for_anchor {
                                Nav::NextSkipExtras
                            } else {
                                Nav::NextSkip
                            })
                        } else {
                            None
                        }
                    } else if !is_first_pattern {
                        Some(Nav::Next)
                    } else {
                        None
                    };

                    result.push((idx, nav));
                    pending_anchor_exact = None;
                    prev_is_anonymous = current_is_anonymous;
                    is_first_pattern = false;
                }
            }
        }

        result
    }
}
