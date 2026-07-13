//! Generic Rust surface for generated query types.
//!
//! Generated modules provide inherent methods as the primary API. These traits
//! and helpers are the generic door: callers can name a query type parameter
//! and still get the same safe `parse`/`matches` behavior.

use crate::{LimitExceeded, Tree};

pub trait Matches {
    fn matches(tree: &Tree, source: &str) -> Result<bool, LimitExceeded>;
}

pub trait Parse<'t, 's>: Matches + Sized {
    fn parse(tree: &'t Tree, source: &'s str) -> Result<Option<Self>, LimitExceeded>;
}

pub fn parse<'t, 's, Q: Parse<'t, 's>>(
    tree: &'t Tree,
    source: &'s str,
) -> Result<Option<Q>, LimitExceeded> {
    Q::parse(tree, source)
}

pub fn matches<Q: Matches>(tree: &Tree, source: &str) -> Result<bool, LimitExceeded> {
    Q::matches(tree, source)
}
