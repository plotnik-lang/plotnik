//! Generic Rust surface for generated query types.
//!
//! Generated modules provide inherent methods as the primary API. These traits
//! and helpers are the generic door: callers can name a query type parameter
//! and use the same supplied-node execution and lazy subtree search.

use std::iter::FusedIterator;

use crate::{LimitExceeded, Node};

pub trait Execute<'t, 's> {
    type Item;

    fn exec(node: Node<'t>, source: &'s str) -> Result<Option<Self::Item>, LimitExceeded>;

    fn each(
        scope: Node<'t>,
        source: &'s str,
    ) -> impl FusedIterator<Item = Result<Self::Item, LimitExceeded>>;

    fn find(scope: Node<'t>, source: &'s str) -> Result<Option<Self::Item>, LimitExceeded> {
        Self::each(scope, source).next().transpose()
    }

    fn find_all(scope: Node<'t>, source: &'s str) -> Result<Vec<Self::Item>, LimitExceeded> {
        Self::each(scope, source).collect::<Result<Vec<_>, _>>()
    }
}

pub fn exec<'t, 's, Q: Execute<'t, 's>>(
    node: Node<'t>,
    source: &'s str,
) -> Result<Option<Q::Item>, LimitExceeded> {
    Q::exec(node, source)
}

pub fn each<'t, 's, Q: Execute<'t, 's>>(
    scope: Node<'t>,
    source: &'s str,
) -> impl FusedIterator<Item = Result<Q::Item, LimitExceeded>> {
    Q::each(scope, source)
}

pub fn find<'t, 's, Q: Execute<'t, 's>>(
    scope: Node<'t>,
    source: &'s str,
) -> Result<Option<Q::Item>, LimitExceeded> {
    Q::find(scope, source)
}

pub fn find_all<'t, 's, Q: Execute<'t, 's>>(
    scope: Node<'t>,
    source: &'s str,
) -> Result<Vec<Q::Item>, LimitExceeded> {
    Q::find_all(scope, source)
}
