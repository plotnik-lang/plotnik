use std::iter::FusedIterator;

use tree_sitter::{Node, TreeCursor};

use crate::LimitExceeded;

/// Inclusive, left-to-right depth-first preorder over a supplied subtree.
///
/// The executor applies an eligible entry once per candidate, using a separate
/// cursor rooted at that candidate. Its result must
/// own its state, not borrow reusable matcher scratch. A successful candidate
/// prunes its descendants. No work on the next candidate happens until the
/// consumer asks for another item.
///
/// The first failure is yielded once, then the iterator is permanently exhausted.
/// `next().transpose()` takes the first match, and fallible collection cannot
/// mistake a partial result vector for success.
pub struct Traversal<'t, F> {
    cursor: TreeCursor<'t>,
    apply: F,
    started: bool,
    matched: bool,
    done: bool,
}

impl<'t, F> Traversal<'t, F> {
    pub fn new(scope: Node<'t>, apply: F) -> Self {
        Self {
            cursor: scope.walk(),
            apply,
            started: false,
            matched: false,
            done: false,
        }
    }

    fn advance(&mut self) -> bool {
        if !self.matched && self.cursor.goto_first_child() {
            return true;
        }

        loop {
            if self.cursor.goto_next_sibling() {
                return true;
            }

            if !self.cursor.goto_parent() {
                return false;
            }
        }
    }
}

impl<'t, T, F> Iterator for Traversal<'t, F>
where
    F: FnMut(Node<'t>) -> Result<Option<T>, LimitExceeded>,
{
    type Item = Result<T, LimitExceeded>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }

        loop {
            let candidate = if self.started {
                self.advance()
            } else {
                self.started = true;
                true
            };
            if !candidate {
                self.done = true;
                return None;
            }

            match (self.apply)(self.cursor.node()) {
                Ok(Some(value)) => {
                    self.matched = true;
                    return Some(Ok(value));
                }
                Ok(None) => {
                    self.matched = false;
                    continue;
                }
                Err(error) => {
                    self.done = true;
                    return Some(Err(error));
                }
            }
        }
    }
}

impl<'t, T, F> FusedIterator for Traversal<'t, F> where
    F: FnMut(Node<'t>) -> Result<Option<T>, LimitExceeded>
{
}
