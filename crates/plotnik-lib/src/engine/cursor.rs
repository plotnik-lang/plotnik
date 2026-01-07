//! TreeCursor wrapper with Plotnik navigation semantics.
//!
//! The wrapper handles the search loop and skip policies defined
//! in docs/tree-navigation.md.

use std::num::NonZeroU16;

use arborium_tree_sitter::{Node, TreeCursor};

use crate::bytecode::Nav;

/// Skip policy for navigation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipPolicy {
    /// Skip any nodes until match.
    Any,
    /// Skip trivia only (fail if non-trivia must be skipped).
    Trivia,
    /// No skipping allowed (exact match required).
    Exact,
}

/// Exit constraint for Up navigation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpMode {
    /// No constraint - just ascend.
    Any,
    /// Must be at last non-trivia child before ascending.
    SkipTrivia,
    /// Must be at last child before ascending.
    Exact,
}

/// Wrapper around TreeCursor with Plotnik navigation semantics.
///
/// Critical: The cursor is created at tree root and never reset.
/// The `descendant_index` is relative to this root, enabling O(1)
/// checkpoint saves and O(depth) restores.
pub struct CursorWrapper<'t> {
    cursor: TreeCursor<'t>,
    /// Trivia node type IDs (for skip policies).
    trivia_types: Vec<u16>,
}

impl<'t> CursorWrapper<'t> {
    /// Create a wrapper around a tree cursor.
    ///
    /// `trivia_types` is the list of node type IDs considered trivia
    /// (e.g., comments, whitespace).
    pub fn new(cursor: TreeCursor<'t>, trivia_types: Vec<u16>) -> Self {
        Self {
            cursor,
            trivia_types,
        }
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

    /// Get the field ID of the current node (if any).
    #[inline]
    pub fn field_id(&self) -> Option<NonZeroU16> {
        self.cursor.field_id()
    }

    /// Get current tree depth (root is 0).
    #[inline]
    pub fn depth(&self) -> u32 {
        self.cursor.depth()
    }

    /// Move cursor to parent node.
    #[inline]
    pub fn goto_parent(&mut self) -> bool {
        self.cursor.goto_parent()
    }

    /// Check if a node type is trivia.
    #[inline]
    pub fn is_trivia(&self, node: &Node<'_>) -> bool {
        // Anonymous nodes are typically trivia (punctuation, operators)
        !node.is_named() || self.trivia_types.contains(&node.kind_id())
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
            Nav::DownExact => self.go_first_child().then_some(SkipPolicy::Exact),
            Nav::Next => self.go_next_sibling().then_some(SkipPolicy::Any),
            Nav::NextSkip => self.go_next_sibling().then_some(SkipPolicy::Trivia),
            Nav::NextExact => self.go_next_sibling().then_some(SkipPolicy::Exact),
            Nav::Up(n) => self.go_up(n, UpMode::Any).then_some(SkipPolicy::Any),
            Nav::UpSkipTrivia(n) => self.go_up(n, UpMode::SkipTrivia).then_some(SkipPolicy::Any),
            Nav::UpExact(n) => self.go_up(n, UpMode::Exact).then_some(SkipPolicy::Any),
        }
    }

    /// Move to first child.
    fn go_first_child(&mut self) -> bool {
        self.cursor.goto_first_child()
    }

    /// Move to next sibling.
    fn go_next_sibling(&mut self) -> bool {
        self.cursor.goto_next_sibling()
    }

    /// Ascend n levels with exit constraint.
    fn go_up(&mut self, levels: u8, mode: UpMode) -> bool {
        // Check exit constraint before ascending
        match mode {
            UpMode::Any => {}
            UpMode::Exact => {
                // Must be at last child
                if self.cursor.goto_next_sibling() {
                    // Oops, there was a next sibling - restore position
                    self.cursor.goto_previous_sibling();
                    return false;
                }
            }
            UpMode::SkipTrivia => {
                // Must be at last non-trivia child
                // Save position
                let saved = self.cursor.descendant_index();

                // Look for non-trivia siblings after us
                while self.cursor.goto_next_sibling() {
                    if !self.is_trivia(&self.cursor.node()) {
                        // Found non-trivia sibling - fail
                        self.cursor.goto_descendant(saved);
                        return false;
                    }
                }
                // Restore position
                self.cursor.goto_descendant(saved);
            }
        }

        // Ascend n levels
        for _ in 0..levels {
            if !self.cursor.goto_parent() {
                return false;
            }
        }
        true
    }

    /// Continue searching for a match with the given skip policy.
    ///
    /// This is called when a match attempt fails. It advances to the next
    /// sibling based on the skip policy and returns whether to retry.
    ///
    /// - `Exact`: Return false (no skipping allowed)
    /// - `Trivia`: Skip trivia siblings only, fail if non-trivia
    /// - `Any`: Skip any siblings
    pub fn continue_search(&mut self, policy: SkipPolicy) -> bool {
        match policy {
            SkipPolicy::Exact => false,
            SkipPolicy::Trivia => {
                // Fail if current node is non-trivia (we'd have to skip it)
                if !self.is_trivia(&self.cursor.node()) {
                    return false;
                }
                self.cursor.goto_next_sibling()
            }
            SkipPolicy::Any => self.cursor.goto_next_sibling(),
        }
    }
}
