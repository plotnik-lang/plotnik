use std::iter::FusedIterator;

use crate::cursor::checked::CheckedCursor;
use tree_sitter::Node;

use crate::alloc::{Account, Buffer, Meter};
use crate::result::{Match, Matches, Publish, Publisher};
use crate::{Engine, LimitExceeded, RuntimeLimitSpec};

/// One compiled entrypoint, implemented by generated matcher modules.
#[doc(hidden)]
pub trait Entry {
    /// The limit policy compiled into the module, resolved per operation.
    const LIMITS: RuntimeLimitSpec;

    /// Panic unless the tree's language matches the generation-time grammar.
    fn verify_language(node: Node<'_>);

    /// Match the engine's current candidate. `true` means accepted, with the
    /// committed output in [`Engine::accepted_tape`].
    fn run<'t>(
        engine: &mut Engine<'t>,
        meter: &mut Meter<'_>,
        source: &str,
    ) -> Result<bool, LimitExceeded>;
}

fn account<M: Entry>(scope: Node<'_>) -> Result<Account, LimitExceeded> {
    let nodes = u32::try_from(scope.descendant_count()).unwrap_or(u32::MAX);
    Account::new(M::LIMITS.resolve(nodes))
}

/// Inclusive, left-to-right depth-first preorder over a supplied subtree.
///
/// A successful candidate prunes its descendants. Every step is charged
/// before it moves the cursor.
#[doc(hidden)]
pub struct Search<'t> {
    cursor: CheckedCursor<'t>,
    started: bool,
    matched: bool,
}

impl<'t> Search<'t> {
    pub fn new(scope: Node<'t>) -> Self {
        Self {
            cursor: CheckedCursor::new(scope),
            started: false,
            matched: false,
        }
    }

    pub fn accepted(&mut self) {
        self.matched = true;
    }

    pub fn next(&mut self, meter: &mut Meter<'_>) -> Result<Option<Node<'t>>, LimitExceeded> {
        if !self.started {
            self.started = true;
            meter.charge(1)?;
            return Ok(Some(self.cursor.node()));
        }
        if !self.matched {
            meter.charge(1)?;
            if self.cursor.first_child(meter)? {
                return Ok(Some(self.cursor.node()));
            }
        }
        self.matched = false;

        loop {
            meter.charge(1)?;
            if self.cursor.next_sibling(meter)? {
                return Ok(Some(self.cursor.node()));
            }

            meter.charge(1)?;
            if !self.cursor.parent(meter)? {
                return Ok(None);
            }
        }
    }
}

/// Apply an entrypoint once at `node`.
#[doc(hidden)]
pub fn exec<'t, 's, E: Publish<'t, 's>, M: Entry>(
    node: Node<'t>,
    source: &'s str,
) -> Result<Option<Match<E>>, LimitExceeded> {
    let account = account::<M>(node)?;
    M::verify_language(node);
    let mut meter = account.meter();
    let mut engine = Engine::new(node);
    if !M::run(&mut engine, &mut meter, source)? {
        return Ok(None);
    }
    Publisher::new()
        .publish(&mut meter, engine.accepted_tape(), source)
        .map(Some)
}

/// Apply a match-only entrypoint once at `node`.
#[doc(hidden)]
pub fn exec_node<'t, M: Entry>(
    node: Node<'t>,
    source: &str,
) -> Result<Option<Node<'t>>, LimitExceeded> {
    let account = account::<M>(node)?;
    M::verify_language(node);
    let mut meter = account.meter();
    let mut engine = Engine::new_match_only(node);
    Ok(M::run(&mut engine, &mut meter, source)?.then_some(node))
}

/// The state of a search that has not failed.
struct Running<'t, 's> {
    search: Search<'t>,
    engine: Engine<'t>,
    publisher: Publisher<'t>,
    source: &'s str,
}

impl<'t, 's> Running<'t, 's> {
    fn new(scope: Node<'t>, source: &'s str, engine: Engine<'t>) -> Self {
        Self {
            search: Search::new(scope),
            engine,
            publisher: Publisher::new(),
            source,
        }
    }

    /// Advance to the next accepted candidate.
    fn accept<M: Entry>(
        &mut self,
        meter: &mut Meter<'_>,
    ) -> Result<Option<Node<'t>>, LimitExceeded> {
        while let Some(node) = self.search.next(meter)? {
            self.engine.reset(node);
            if M::run(&mut self.engine, meter, self.source)? {
                self.search.matched = true;
                return Ok(Some(node));
            }
        }
        Ok(None)
    }

    fn publish<E: Publish<'t, 's>>(
        &mut self,
        meter: &mut Meter<'_>,
    ) -> Result<Match<E>, LimitExceeded> {
        self.publisher
            .publish(meter, self.engine.accepted_tape(), self.source)
    }
}

/// Lazy search over a subtree, yielding independently owned matches.
///
/// The fuel permit is held only inside `next`. The first failure is yielded
/// once, then the iterator is exhausted.
#[doc(hidden)]
pub struct Each<'t, 's, T, M> {
    account: Option<Account>,
    running: Option<Running<'t, 's>>,
    failure: Option<LimitExceeded>,
    produce: fn(&mut Running<'t, 's>, &mut Meter<'_>, Node<'t>) -> Result<T, LimitExceeded>,
    _entry: std::marker::PhantomData<M>,
}

impl<'t, 's, T, M: Entry> Each<'t, 's, T, M> {
    fn start(
        scope: Node<'t>,
        source: &'s str,
        engine: Engine<'t>,
        produce: fn(&mut Running<'t, 's>, &mut Meter<'_>, Node<'t>) -> Result<T, LimitExceeded>,
    ) -> Self {
        let (account, running, failure) = match account::<M>(scope) {
            Ok(account) => {
                M::verify_language(scope);
                (
                    Some(account),
                    Some(Running::new(scope, source, engine)),
                    None,
                )
            }
            Err(error) => (None, None, Some(error)),
        };
        Self {
            account,
            running,
            failure,
            produce,
            _entry: std::marker::PhantomData,
        }
    }
}

impl<T, M: Entry> Iterator for Each<'_, '_, T, M> {
    type Item = Result<T, LimitExceeded>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(error) = self.failure.take() {
            return Some(Err(error));
        }
        let account = self.account.as_ref()?;
        let running = self.running.as_mut()?;

        let mut meter = account.meter();
        let produced = match running.accept::<M>(&mut meter) {
            Ok(Some(node)) => (self.produce)(running, &mut meter, node).map(Some),
            Ok(None) => Ok(None),
            Err(error) => Err(error),
        };
        drop(meter);

        match produced {
            Ok(Some(item)) => Some(Ok(item)),
            Ok(None) => {
                self.running = None;
                None
            }
            Err(error) => {
                self.running = None;
                Some(Err(error))
            }
        }
    }
}

impl<T, M: Entry> FusedIterator for Each<'_, '_, T, M> {}

/// Search a subtree, yielding owned matches.
#[doc(hidden)]
pub fn each<'t, 's, E: Publish<'t, 's>, M: Entry>(
    scope: Node<'t>,
    source: &'s str,
) -> Each<'t, 's, Match<E>, M> {
    Each::start(scope, source, Engine::new(scope), |running, meter, _| {
        running.publish(meter)
    })
}

/// Search a subtree with a match-only entrypoint, yielding matched nodes.
#[doc(hidden)]
pub fn each_node<'t, 's, M: Entry>(scope: Node<'t>, source: &'s str) -> Each<'t, 's, Node<'t>, M> {
    Each::start(
        scope,
        source,
        Engine::new_match_only(scope),
        |_, _, node| Ok(node),
    )
}

/// Collect every match of a subtree search, or fail without partial results.
#[doc(hidden)]
pub fn find_all<'t, 's, E: Publish<'t, 's>, M: Entry>(
    scope: Node<'t>,
    source: &'s str,
) -> Result<Matches<E>, LimitExceeded> {
    let account = account::<M>(scope)?;
    M::verify_language(scope);
    let mut meter = account.meter();
    let mut running = Running::new(scope, source, Engine::new(scope));
    let mut matches = Matches::new();
    while running.accept::<M>(&mut meter)?.is_some() {
        let item = running.publish(&mut meter)?;
        matches.push(&mut meter, item)?;
    }
    Ok(matches)
}

/// Collect every node a match-only entrypoint accepts in a subtree search.
#[doc(hidden)]
pub fn find_all_nodes<'t, M: Entry>(
    scope: Node<'t>,
    source: &str,
) -> Result<Buffer<Node<'t>>, LimitExceeded> {
    let account = account::<M>(scope)?;
    M::verify_language(scope);
    let mut meter = account.meter();
    let mut running = Running::new(scope, source, Engine::new_match_only(scope));
    let mut nodes = Buffer::new();
    while let Some(node) = running.accept::<M>(&mut meter)? {
        nodes.push(&mut meter, node)?;
    }
    Ok(nodes)
}
