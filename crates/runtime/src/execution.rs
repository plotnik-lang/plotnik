use std::iter::FusedIterator;

use tree_sitter::{Node, TreeCursor};

use crate::{LimitExceeded, ResolvedRuntimeLimits};

const MEMORY_SAMPLE_INTERVAL: u64 = 1024;

/// One budget shared by traversal and every candidate application.
/// Limits are resolved once by the executor from its existing policy and scope.
/// This state is not rollbackable and must not be recreated on backtracking.
pub struct ExecutionBudget {
    limits: ResolvedRuntimeLimits,
    fuel_used: u64,
    memory_sample_at: u64,
}

impl ExecutionBudget {
    pub fn new(limits: ResolvedRuntimeLimits) -> Self {
        Self {
            limits,
            fuel_used: 0,
            memory_sample_at: 0,
        }
    }

    pub fn limits(&self) -> ResolvedRuntimeLimits {
        self.limits
    }

    pub fn fuel_used(&self) -> u64 {
        self.fuel_used
    }

    /// Preserve the runtime's memory sampling interval across applications.
    /// Traversal can cross a sample boundary between matcher dispatches, so an
    /// equality or bitmask check on the cumulative counter can miss it.
    pub fn memory_sample_due(&mut self) -> bool {
        if self.fuel_used - self.memory_sample_at < MEMORY_SAMPLE_INTERVAL {
            return false;
        }

        self.memory_sample_at = self.fuel_used;
        true
    }

    /// Charge one matcher dispatch or traversal step before doing the work.
    pub fn charge(&mut self) -> Result<(), LimitExceeded> {
        if let Some(limit) = self.limits.fuel_limit
            && self.fuel_used >= limit
        {
            return Err(LimitExceeded::OutOfFuel(limit));
        }

        self.fuel_used += 1;
        Ok(())
    }
}

/// Inclusive, left-to-right depth-first preorder over a supplied subtree.
///
/// The executor applies an eligible entry once per candidate, using a separate
/// cursor rooted at that candidate and this operation's budget. Its result must
/// own its state, not borrow reusable matcher scratch. A successful candidate
/// does not prune descendants. No work on the next candidate happens until the
/// consumer asks for another item.
///
/// The first failure is yielded once, then the iterator is permanently exhausted.
/// `next().transpose()` takes the first match, and fallible collection cannot
/// mistake a partial result vector for success.
pub struct Traversal<'t, F> {
    cursor: TreeCursor<'t>,
    budget: ExecutionBudget,
    apply: F,
    started: bool,
    done: bool,
}

impl<'t, F> Traversal<'t, F> {
    pub fn new(scope: Node<'t>, budget: ExecutionBudget, apply: F) -> Self {
        Self {
            cursor: scope.walk(),
            budget,
            apply,
            started: false,
            done: false,
        }
    }

    fn advance(&mut self) -> Result<bool, LimitExceeded> {
        self.budget.charge()?;
        if self.cursor.goto_first_child() {
            return Ok(true);
        }

        loop {
            self.budget.charge()?;
            if self.cursor.goto_next_sibling() {
                return Ok(true);
            }

            self.budget.charge()?;
            if !self.cursor.goto_parent() {
                return Ok(false);
            }
        }
    }
}

impl<'t, T, F> Iterator for Traversal<'t, F>
where
    F: FnMut(Node<'t>, &mut ExecutionBudget) -> Result<Option<T>, LimitExceeded>,
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
                self.budget.charge().map(|()| true)
            };
            let result = match candidate {
                Ok(true) => (self.apply)(self.cursor.node(), &mut self.budget),
                Ok(false) => {
                    self.done = true;
                    return None;
                }
                Err(error) => Err(error),
            };

            match result {
                Ok(Some(value)) => return Some(Ok(value)),
                Ok(None) => continue,
                Err(error) => {
                    self.done = true;
                    return Some(Err(error));
                }
            }
        }
    }
}

impl<'t, T, F> FusedIterator for Traversal<'t, F> where
    F: FnMut(Node<'t>, &mut ExecutionBudget) -> Result<Option<T>, LimitExceeded>
{
}
