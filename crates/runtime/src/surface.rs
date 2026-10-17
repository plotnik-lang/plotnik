use std::iter::FusedIterator;

use crate::{LimitExceeded, Node};

/// The execution points of a generated entrypoint, for generic callers.
///
/// Value entrypoints produce [`Match`](crate::Match) results and collect
/// into [`Matches`](crate::Matches); match-only entrypoints produce the
/// matched `Node`s.
pub trait Execute<'t, 's> {
    type Item;
    type All;

    fn exec(node: Node<'t>, source: &'s str) -> Result<Option<Self::Item>, LimitExceeded>;

    fn each(
        scope: Node<'t>,
        source: &'s str,
    ) -> impl FusedIterator<Item = Result<Self::Item, LimitExceeded>>;

    fn find(scope: Node<'t>, source: &'s str) -> Result<Option<Self::Item>, LimitExceeded> {
        Self::each(scope, source).next().transpose()
    }

    fn find_all(scope: Node<'t>, source: &'s str) -> Result<Self::All, LimitExceeded>;
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
) -> Result<Q::All, LimitExceeded> {
    Q::find_all(scope, source)
}
