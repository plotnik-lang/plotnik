//! Shared anchor truth: the one place sibling-gap semantics are computed.
//!
//! Both codegen (`lower/thompson`) and the grammar satisfiability checker
//! (`analyze/grammar/satisfy`) must agree on exactly what an anchor lets a gap
//! skip — if they drift, the checker rejects what the VM would have matched (or
//! the reverse). So the nav computation lives here, in `analyze`, where both may
//! depend on it, and codegen re-exports it rather than forking it. [`GapClass`]
//! projects those navs onto the skip classes the checker reasons over; its
//! `admits` truth table mirrors the VM's skip policy (`vm/engine/cursor.rs`) by
//! construction.

use std::collections::HashSet;

use crate::bytecode::Nav;
use crate::compiler::analyze::names::SymbolTable;
use crate::compiler::parse::ast::{DefRef, Pattern, SeqItem};

/// What a gap between two query patterns may skip over, projected from the same
/// [`Nav`] codegen emits so the checker and the VM cannot drift.
///
/// The axes mirror the VM exactly (`vm/engine/cursor.rs`): a node is *anonymous*
/// when it is not named and *extra* when the parser marked it as an extra (a
/// comment, say). The two are independent — a named comment is `(false, true)`, an
/// anonymous brace `(true, false)`. A broad skip clears `anonymous || extra` (the
/// VM's `is_trivia`); a narrow skip clears only `extra`.
// The grammar satisfiability checker (`analyze/grammar/satisfy`, Stage B) is the
// consumer; it lands on top of this shared truth, so the type is dead until then.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GapClass {
    /// No anchor: any node may sit in the gap (the VM's `SkipPolicy::Any`).
    Any,
    /// Soft `.` with both operands named: skip anonymous tokens and extras.
    AnonAndExtras,
    /// Soft `.` with an anonymous operand: skip extras only.
    ExtrasOnly,
    /// Strict `.!`: nothing may intervene.
    Nothing,
}

#[allow(dead_code)]
impl GapClass {
    /// Whether a node carrying these class bits may be skipped across this gap.
    /// This is the VM's skip policy, by construction (`cursor.rs`'s `is_trivia` is
    /// `anonymous || extra`, `SkipExtras` is `extra`).
    pub fn admits(self, anonymous: bool, extra: bool) -> bool {
        match self {
            Self::Any => true,
            Self::AnonAndExtras => anonymous || extra,
            Self::ExtrasOnly => extra,
            Self::Nothing => false,
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
            Nav::NextSkip | Nav::DownSkip | Nav::UpSkipTrivia(_) => Self::AnonAndExtras,
            Nav::NextSkipExtras | Nav::DownSkipExtras | Nav::UpSkipExtras(_) => Self::ExtrasOnly,
            Nav::NextExact | Nav::DownExact | Nav::UpExact(_) => Self::Nothing,
            Nav::Epsilon | Nav::Stay | Nav::StayExact => return None,
        };
        Some(class)
    }
}

/// Classifies whether patterns may match anonymous nodes after syntactic wrappers.
pub struct AnonymousClassifier<'a> {
    symbol_table: &'a SymbolTable,
}

fn pattern_has_direct_alt_branch_nav(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::Union(_) | Pattern::Enum(_) => true,
        Pattern::CapturedPattern(cap) => cap
            .inner()
            .as_ref()
            .is_some_and(pattern_has_direct_alt_branch_nav),
        _ => false,
    }
}

impl<'a> AnonymousClassifier<'a> {
    pub fn new(symbol_table: &'a SymbolTable) -> Self {
        Self { symbol_table }
    }

    pub fn pattern_may_match_anonymous(&self, pattern: Option<&Pattern>) -> bool {
        let mut visited = HashSet::new();
        pattern.is_some_and(|pattern| self.pattern_may_match_anonymous_inner(pattern, &mut visited))
    }

    fn pattern_may_match_anonymous_inner(
        &self,
        pattern: &Pattern,
        visited: &mut HashSet<String>,
    ) -> bool {
        match pattern {
            Pattern::TokenPattern(_) => true,
            Pattern::CapturedPattern(cap) => cap
                .inner()
                .as_ref()
                .is_some_and(|inner| self.pattern_may_match_anonymous_inner(inner, visited)),
            Pattern::QuantifiedPattern(q) => q
                .inner()
                .as_ref()
                .is_some_and(|inner| self.pattern_may_match_anonymous_inner(inner, visited)),
            Pattern::FieldPattern(field) => field
                .value()
                .as_ref()
                .is_some_and(|value| self.pattern_may_match_anonymous_inner(value, visited)),
            Pattern::Union(_) | Pattern::Enum(_) => pattern
                .children()
                .any(|body| self.pattern_may_match_anonymous_inner(&body, visited)),
            Pattern::SeqPattern(seq) => seq
                .children()
                .any(|child| self.pattern_may_match_anonymous_inner(&child, visited)),
            Pattern::DefRef(r) => self.ref_may_match_anonymous(r, visited),
            Pattern::NodePattern(_) => false,
        }
    }

    fn ref_may_match_anonymous(&self, r: &DefRef, visited: &mut HashSet<String>) -> bool {
        let Some(name_token) = r.name() else {
            return false;
        };
        let name = name_token.text();

        if !visited.insert(name.to_owned()) {
            return false;
        }

        let result = self
            .symbol_table
            .body(name)
            .is_some_and(|body| self.pattern_may_match_anonymous_inner(body, visited));

        visited.remove(name);
        result
    }
}

/// Check for trailing anchor in items, descending into a sole-child sequence if needed.
pub fn check_trailing_anchor(items: &[SeqItem], symbol_table: &SymbolTable) -> (bool, Option<Nav>) {
    if let Some(SeqItem::Anchor(anchor)) = items.last() {
        if anchor.is_strict() {
            return (true, Some(Nav::UpExact(1)));
        }

        let prev_pattern = items.iter().rev().skip(1).find_map(|item| {
            if let SeqItem::Pattern(e) = item {
                Some(e)
            } else {
                None
            }
        });

        let classifier = AnonymousClassifier::new(symbol_table);
        let nav = if classifier.pattern_may_match_anonymous(prev_pattern) {
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
        return check_trailing_anchor(&seq_items, symbol_table);
    }

    (false, None)
}

pub fn compute_nav_modes(
    items: &[SeqItem],
    is_inside_node: bool,
    symbol_table: &SymbolTable,
) -> Vec<(usize, Option<Nav>)> {
    let mut result = Vec::new();
    let mut pending_anchor_strict = None;
    let mut prev_is_anonymous = false;
    let mut is_first_pattern = true;
    let classifier = AnonymousClassifier::new(symbol_table);

    for (idx, item) in items.iter().enumerate() {
        match item {
            SeqItem::Anchor(anchor) => {
                pending_anchor_strict = Some(anchor.is_strict());
            }
            SeqItem::Pattern(pattern) => {
                let current_is_anonymous = classifier.pattern_may_match_anonymous(Some(pattern));
                // Alternation branches compile their own entry nav, so the branch body—not
                // the whole alternation—decides whether soft anchors use extras-only nav.
                let current_is_anonymous_for_anchor = if pattern_has_direct_alt_branch_nav(pattern)
                {
                    false
                } else {
                    current_is_anonymous
                };
                let nav = if let Some(is_exact) = pending_anchor_strict {
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
                pending_anchor_strict = None;
                prev_is_anonymous = current_is_anonymous;
                is_first_pattern = false;
            }
        }
    }

    result
}
