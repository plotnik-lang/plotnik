//! TreeCursor wrapper with Plotnik navigation semantics.
//!
//! The wrapper handles the search loop and skip policies defined
//! in docs/tree-navigation.md.

use std::num::NonZeroU16;

use arborium_tree_sitter::{Node, TreeCursor};

use plotnik_bytecode::Nav;

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

    /// Get the current node.
    #[inline]
    pub fn node(&self) -> Node<'t> {
        self.cursor.node()
    }

    /// Get the current cursor position for checkpointing.
    #[inline]
    pub fn descendant_index(&self) -> u32 {
        self.cursor.descendant_index() as u32
    }

    /// Restore cursor to a checkpointed position.
    #[inline]
    pub fn goto_descendant(&mut self, index: u32) {
        self.cursor.goto_descendant(index as usize);
    }

    #[inline]
    pub fn field_id(&self) -> Option<NonZeroU16> {
        self.cursor.field_id()
    }

    /// Get current tree depth (root is 0).
    #[inline]
    pub fn depth(&self) -> u32 {
        self.cursor.depth()
    }

    #[inline]
    pub fn goto_parent(&mut self) -> bool {
        self.cursor.goto_parent()
    }

    /// TODO: when extracting a common tree-sitter wrapper (arborium vs vanilla tree-sitter),
    ///       give `Node` an `is_trivia()` method so n.is_trivia(), n.is_named(), and
    ///       n.is_extra() are uniform.
    #[inline]
    pub fn is_trivia(node: &Node<'_>) -> bool {
        // Anonymous skipping is documented anchor semantics; `is_extra` is the
        // parser's per-instance bit, so the same kind can be extra in one
        // position and structural in another.
        !node.is_named() || node.is_extra()
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
            UpMode::SkipExtras => self.is_last_child_skipping(|n| n.is_extra()),
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
            SkipPolicy::Trivia => {
                if !Self::is_trivia(&self.cursor.node()) {
                    return false;
                }
                self.cursor.goto_next_sibling()
            }
            SkipPolicy::Extras => {
                if !self.cursor.node().is_extra() {
                    return false;
                }
                self.cursor.goto_next_sibling()
            }
            SkipPolicy::Any => self.cursor.goto_next_sibling(),
        }
    }
}
