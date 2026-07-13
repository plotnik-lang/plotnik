//! Generic Rust surface for generated query types.
//!
//! Generated modules provide inherent methods as the primary API. These traits
//! and helpers are the generic door: callers can name a query type parameter
//! and still get the same safe `match_tree`/`is_match` behavior.

use crate::{LimitExceeded, Tree};

pub trait IsMatch {
    fn is_match(tree: &Tree, source: &str) -> Result<bool, LimitExceeded>;
}

pub trait MatchTree<'t, 's>: IsMatch + Sized {
    fn match_tree(tree: &'t Tree, source: &'s str) -> Result<Option<Self>, LimitExceeded>;
}

pub fn match_tree<'t, 's, Q: MatchTree<'t, 's>>(
    tree: &'t Tree,
    source: &'s str,
) -> Result<Option<Q>, LimitExceeded> {
    Q::match_tree(tree, source)
}

pub fn is_match<Q: IsMatch>(tree: &Tree, source: &str) -> Result<bool, LimitExceeded> {
    Q::is_match(tree, source)
}
