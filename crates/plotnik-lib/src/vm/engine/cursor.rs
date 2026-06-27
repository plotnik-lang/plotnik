//! TreeCursor wrapper with Plotnik navigation semantics.
//!
//! The wrapper handles the search loop and skip policies defined
//! in docs/tree-navigation.md.

use arborium_tree_sitter::{Node, TreeCursor};

use crate::bytecode::Nav;
use crate::core::{NodeClass, NodeFieldId, SkipClass};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipPolicy {
    /// Skip any nodes until match.
    Any,
    /// Skip trivia only (fail if non-trivia must be skipped).
    Trivia,
    /// Skip tree-sitter extras only (fail if a regular anonymous token must be skipped).
    Extras,
    /// No skipping allowed (exact match required).
    Exact,
}

impl SkipPolicy {
    fn skip_class(self) -> SkipClass {
        match self {
            Self::Any => SkipClass::Any,
            Self::Trivia => SkipClass::Trivia,
            Self::Extras => SkipClass::Extras,
            Self::Exact => SkipClass::Exact,
        }
    }

    fn admits(self, node: &Node<'_>) -> bool {
        self.skip_class().admits(CursorWrapper::node_class(node))
    }
}

/// Exit constraint for Up navigation, checked at *each* level ascended (so
/// same-mode `Up*` instructions compose; see [`CursorWrapper::go_up`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpMode {
    /// No constraint - just ascend.
    Any,
    /// Each node left must be its parent's last non-trivia child.
    SkipTrivia,
    /// Each node left must be its parent's last non-extra child.
    SkipExtras,
    /// Each node left must be its parent's last child.
    Exact,
}

impl UpMode {
    fn skip_class(self) -> SkipClass {
        match self {
            Self::Any => SkipClass::Any,
            Self::SkipTrivia => SkipClass::Trivia,
            Self::SkipExtras => SkipClass::Extras,
            Self::Exact => SkipClass::Exact,
        }
    }
}

/// Wrapper around TreeCursor with Plotnik navigation semantics.
///
/// Critical: The cursor is created at tree root and never reset.
/// The `descendant_index` is relative to this root, enabling O(1)
/// checkpoint saves and O(depth) restores.
pub struct CursorWrapper<'t> {
    cursor: TreeCursor<'t>,
}

impl<'t> CursorWrapper<'t> {
    pub fn new(cursor: TreeCursor<'t>) -> Self {
        Self { cursor }
    }

    #[inline]
    pub fn node(&self) -> Node<'t> {
        self.cursor.node()
    }

    #[inline]
    pub fn descendant_index(&self) -> u32 {
        self.cursor.descendant_index() as u32
    }

    #[inline]
    pub fn goto_descendant(&mut self, index: u32) {
        self.cursor.goto_descendant(index as usize);
    }

    #[inline]
    pub fn field_id(&self) -> Option<NodeFieldId> {
        self.cursor.field_id().map(NodeFieldId::from)
    }

    #[inline]
    pub fn depth(&self) -> u32 {
        self.cursor.depth()
    }

    #[inline]
    pub fn goto_parent(&mut self) -> bool {
        self.cursor.goto_parent()
    }

    #[inline]
    fn node_class(node: &Node<'_>) -> NodeClass {
        NodeClass::from_runtime(node.is_named(), node.is_extra())
    }

    #[inline]
    pub fn is_trivia(node: &Node<'_>) -> bool {
        SkipClass::Trivia.admits(Self::node_class(node))
    }

    /// Navigate according to Nav command, preparing for match attempt.
    ///
    /// Returns the skip policy to use for the subsequent match attempt,
    /// or None if navigation failed (no children/siblings).
    pub fn navigate(&mut self, nav: Nav) -> Option<SkipPolicy> {
        match nav {
            // Epsilon should never reach here - VM skips navigate for epsilon
            Nav::Epsilon => {
                debug_assert!(
                    false,
                    "navigate called with Epsilon - should be skipped by VM"
                );
                Some(SkipPolicy::Any)
            }
            Nav::Stay => Some(SkipPolicy::Any),
            Nav::StayExact => Some(SkipPolicy::Exact),
            Nav::Down => self.go_first_child().then_some(SkipPolicy::Any),
            Nav::DownSkip => self.go_first_child().then_some(SkipPolicy::Trivia),
            Nav::DownSkipExtras => self.go_first_child().then_some(SkipPolicy::Extras),
            Nav::DownExact => self.go_first_child().then_some(SkipPolicy::Exact),
            Nav::Next => self.go_next_sibling().then_some(SkipPolicy::Any),
            Nav::NextSkip => self.go_next_sibling().then_some(SkipPolicy::Trivia),
            Nav::NextSkipExtras => self.go_next_sibling().then_some(SkipPolicy::Extras),
            Nav::NextExact => self.go_next_sibling().then_some(SkipPolicy::Exact),
            Nav::Up(n) => self.go_up(n, UpMode::Any).then_some(SkipPolicy::Any),
            Nav::UpSkipTrivia(n) => self.go_up(n, UpMode::SkipTrivia).then_some(SkipPolicy::Any),
            Nav::UpSkipExtras(n) => self.go_up(n, UpMode::SkipExtras).then_some(SkipPolicy::Any),
            Nav::UpExact(n) => self.go_up(n, UpMode::Exact).then_some(SkipPolicy::Any),
        }
    }

    fn go_first_child(&mut self) -> bool {
        self.cursor.goto_first_child()
    }

    fn go_next_sibling(&mut self) -> bool {
        self.cursor.goto_next_sibling()
    }

    /// Ascend `levels` levels, validating the exit constraint at *every* level.
    ///
    /// Same-mode `Up*` instructions compose: `Up*(a)` then `Up*(b)` is `Up*(a+b)`,
    /// because each node being exited is checked in turn. This is what makes a
    /// nested trailing anchor — `(array (object (pair) .) .)`, "pair last in
    /// object AND object last in array" — sound when `collapse_up` merges the two
    /// single-level ascents (and why merging caps at the encoding limit rather
    /// than dropping checks; see docs/tree-navigation.md).
    ///
    /// On any failure the cursor is restored to where it started, so a failed
    /// navigation leaves no net movement (the VM also backtracks to a checkpoint,
    /// but keeping this self-contained avoids relying on that).
    fn go_up(&mut self, levels: u8, mode: UpMode) -> bool {
        let origin = self.cursor.descendant_index();
        for _ in 0..levels {
            if !self.exit_constraint_holds(mode) || !self.cursor.goto_parent() {
                self.cursor.goto_descendant(origin);
                return false;
            }
        }
        true
    }

    /// Whether the current node satisfies `mode`'s last-child constraint. Any
    /// sibling probe is undone before returning, so the cursor stays on the
    /// current node either way — ready for the caller to ascend.
    fn exit_constraint_holds(&mut self, mode: UpMode) -> bool {
        match mode {
            UpMode::Any => true,
            UpMode::Exact => {
                // Must be the last child — no next sibling at all.
                if self.cursor.goto_next_sibling() {
                    self.cursor.goto_previous_sibling();
                    return false;
                }
                true
            }
            // Last child once trailing trivia / extras are ignored.
            UpMode::SkipTrivia => self.is_last_child_skipping(Self::is_trivia),
            UpMode::SkipExtras => {
                let skip_class = mode.skip_class();
                self.is_last_child_skipping(|n| skip_class.admits(Self::node_class(n)))
            }
        }
    }

    /// Whether no non-`skippable` sibling follows the current node — i.e. it is
    /// the last child once trailing `skippable` siblings are ignored. The sibling
    /// scan is undone before returning, so the cursor stays on the current node.
    fn is_last_child_skipping(&mut self, skippable: impl Fn(&Node<'t>) -> bool) -> bool {
        let saved = self.cursor.descendant_index();
        while self.cursor.goto_next_sibling() {
            if !skippable(&self.cursor.node()) {
                self.cursor.goto_descendant(saved);
                return false;
            }
        }
        self.cursor.goto_descendant(saved);
        true
    }

    /// Continue searching for a match with the given skip policy.
    ///
    /// This is called when a match attempt fails. It advances to the next
    /// sibling based on the skip policy and returns whether to retry.
    ///
    /// - `Exact`: Return false (no skipping allowed)
    /// - `Trivia`: Skip trivia siblings only, fail if non-trivia
    /// - `Extras`: Skip tree-sitter extras only, fail otherwise
    /// - `Any`: Skip any siblings
    pub fn continue_search(&mut self, policy: SkipPolicy) -> bool {
        match policy {
            SkipPolicy::Exact => false,
            SkipPolicy::Trivia | SkipPolicy::Extras => {
                if !policy.admits(&self.cursor.node()) {
                    return false;
                }
                self.cursor.goto_next_sibling()
            }
            SkipPolicy::Any => self.cursor.goto_next_sibling(),
        }
    }
}
