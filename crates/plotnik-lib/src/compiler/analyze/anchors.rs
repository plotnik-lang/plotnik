//! Shared anchor truth: the one place sibling-gap semantics are computed.
//!
//! Both codegen (`lower/thompson`) and the grammar satisfiability checker
//! (`analyze/grammar/satisfiability`) must agree on exactly what an anchor lets a gap
//! skip — if they drift, the checker rejects what the VM would have matched (or
//! the reverse). So the nav computation lives here, in `analyze`, where both may
//! depend on it, and codegen re-exports it rather than forking it.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use crate::bytecode::Nav;
use crate::compiler::analyze::names::SymbolTable;
use crate::compiler::parse::ast::{DefRef, Pattern, SeqItem};

/// Classifies whether patterns may match anonymous nodes after syntactic wrappers.
pub struct AnonymousClassifier<'a> {
    symbol_table: &'a SymbolTable,
    /// Memoizes each definition's result so a reference-heavy DAG — an alternation
    /// referenced twice per level, say — is walked once per definition, not once per
    /// path: the difference between linear and exponential. Only path-independent
    /// results are stored; see [`AnonymousClassifier::classify_ref`].
    cache: RefCell<HashMap<String, bool>>,
}

/// Computes anchor-derived navigation with one anonymous-pattern cache.
///
/// A node body needs both leading-gap navs and trailing-anchor navs; building those
/// through separate free functions used to create separate classifiers and re-walk
/// the same referenced definitions. Keep the classifier here so one construction pass
/// pays that cost once.
pub struct AnchorSemantics<'a> {
    classifier: AnonymousClassifier<'a>,
}

/// Whether this pattern's immediate branches compile branch-local entry navs.
///
/// A soft anchor before such a pattern is decided by each branch, not by the
/// alternation's whole-pattern anonymous classification.
pub(crate) fn has_direct_alternation_branch_nav(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::Union(_) | Pattern::Enum(_) => true,
        Pattern::CapturedPattern(cap) => cap
            .inner()
            .as_ref()
            .is_some_and(has_direct_alternation_branch_nav),
        _ => false,
    }
}

impl<'a> AnonymousClassifier<'a> {
    pub fn new(symbol_table: &'a SymbolTable) -> Self {
        Self {
            symbol_table,
            cache: RefCell::new(HashMap::new()),
        }
    }

    pub fn pattern_may_match_anonymous(&self, pattern: Option<&Pattern>) -> bool {
        let mut visited = HashSet::new();
        pattern.is_some_and(|pattern| self.classify(pattern, &mut visited).0)
    }

    /// Returns `(may_match, cut)`. `cut` is true when the walk broke a reference cycle:
    /// the answer then hinged on which names were already on the stack, so a `false`
    /// carrying it is not safe to memoize.
    fn classify(&self, pattern: &Pattern, visited: &mut HashSet<String>) -> (bool, bool) {
        match pattern {
            Pattern::TokenPattern(_) => (true, false),
            Pattern::NodePattern(_) => (false, false),
            Pattern::CapturedPattern(cap) => self.classify_opt(cap.inner().as_ref(), visited),
            Pattern::QuantifiedPattern(q) => self.classify_opt(q.inner().as_ref(), visited),
            Pattern::FieldPattern(field) => self.classify_opt(field.value().as_ref(), visited),
            Pattern::Union(_) | Pattern::Enum(_) => self.classify_any(pattern.children(), visited),
            Pattern::SeqPattern(seq) => self.classify_any(seq.children(), visited),
            Pattern::DefRef(r) => self.classify_ref(r, visited),
        }
    }

    fn classify_opt(
        &self,
        pattern: Option<&Pattern>,
        visited: &mut HashSet<String>,
    ) -> (bool, bool) {
        pattern.map_or((false, false), |p| self.classify(p, visited))
    }

    /// OR over children: any branch that may match anonymous makes the whole pattern.
    /// A `true` short-circuits (and is always sound to cache); an all-`false` answer
    /// inherits a cut from any branch, since a cut branch might have masked a `true`.
    fn classify_any(
        &self,
        children: impl Iterator<Item = Pattern>,
        visited: &mut HashSet<String>,
    ) -> (bool, bool) {
        let mut cut = false;
        for child in children {
            let (result, child_cut) = self.classify(&child, visited);
            if result {
                return (true, false);
            }
            cut |= child_cut;
        }
        (false, cut)
    }

    fn classify_ref(&self, r: &DefRef, visited: &mut HashSet<String>) -> (bool, bool) {
        let Some(name_token) = r.name() else {
            return (false, false);
        };
        let name = name_token.text();

        if let Some(&cached) = self.cache.borrow().get(name) {
            return (cached, false);
        }
        if visited.contains(name) {
            // A reference cycle: going around it again adds nothing, but this `false`
            // holds only with the current ancestors on the stack, so flag it uncacheable.
            return (false, true);
        }

        visited.insert(name.to_owned());
        let (result, cut) = self
            .symbol_table
            .body(name)
            .map_or((false, false), |body| self.classify(body, visited));
        visited.remove(name);

        // A `true` is genuine no matter what was cut; a cut-free `false` is path-
        // independent. Either is sound to reuse for every later reference to this name.
        if result || !cut {
            self.cache.borrow_mut().insert(name.to_owned(), result);
        }
        (result, cut)
    }
}

impl<'a> AnchorSemantics<'a> {
    pub fn new(symbol_table: &'a SymbolTable) -> Self {
        Self {
            classifier: AnonymousClassifier::new(symbol_table),
        }
    }

    pub fn pattern_may_match_anonymous(&self, pattern: Option<&Pattern>) -> bool {
        self.classifier.pattern_may_match_anonymous(pattern)
    }

    /// Check for trailing anchor in items, descending into a sole-child sequence if needed.
    pub fn check_trailing_anchor(&self, items: &[SeqItem]) -> (bool, Option<Nav>) {
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

            let nav = if self.classifier.pattern_may_match_anonymous(prev_pattern) {
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

    pub fn compute_nav_modes(
        &self,
        items: &[SeqItem],
        is_inside_node: bool,
    ) -> Vec<(usize, Option<Nav>)> {
        let mut result = Vec::new();
        let mut pending_anchor_strict = None;
        let mut prev_is_anonymous = false;
        let mut is_first_pattern = true;

        for (idx, item) in items.iter().enumerate() {
            match item {
                SeqItem::Anchor(anchor) => {
                    pending_anchor_strict = Some(anchor.is_strict());
                }
                SeqItem::Pattern(pattern) => {
                    let current_is_anonymous =
                        self.classifier.pattern_may_match_anonymous(Some(pattern));
                    // Alternation branches compile their own entry nav, so the branch body—not
                    // the whole alternation—decides whether soft anchors use extras-only nav.
                    let current_is_anonymous_for_anchor =
                        if has_direct_alternation_branch_nav(pattern) {
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
}
