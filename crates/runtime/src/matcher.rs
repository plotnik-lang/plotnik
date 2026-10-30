use crate::frame::CallToken;
use crate::{Engine, JournalEvent, LimitExceeded, MatchJournal, Node, PortId};

/// Generated engine with local block checkpoints and static call descriptors.
#[doc(hidden)]
pub type MatcherEngine<'t> = Engine<'t, &'static Call>;

#[doc(hidden)]
pub type Runner = for<'t> fn(&mut MatcherEngine<'t>, &str, u32) -> Flow;

/// Callee entry and caller-local return blocks.
#[doc(hidden)]
#[derive(Debug)]
pub struct Call {
    pub runner: Runner,
    pub entry: u32,
    pub returns: &'static [u32],
    pub site: u16,
}

impl CallToken for &'static Call {
    type Resume = u32;

    fn diagnostic_site(self) -> u16 {
        self.site
    }
}

/// Grammar ids used by the generated module.
#[doc(hidden)]
pub struct Grammar {
    pub kinds: &'static [(u16, &'static str, bool)],
    pub fields: &'static [(u16, &'static str)],
    pub identity: Option<(&'static str, &'static str, &'static str)>,
}

impl Grammar {
    /// Panic if a listed kind or field differs from the tree's grammar.
    pub fn verify(&self, node: Node<'_>) {
        let language = node.language();
        for &(id, name, named) in self.kinds {
            let found = language.node_kind_for_id(id);
            let found_named = language.node_kind_is_named(id);
            if found == Some(name) && found_named == named {
                continue;
            }
            if let Some((grammar, source, sha256)) = self.identity {
                panic!(
                    "grammar version skew: node kind {id}: expected {name:?} (named={named}), \
                     found {found:?} (named={found_named}). Generated for {grammar} \
                     ({source}, grammar.json SHA-256 {sha256}). \
                     Regenerate with the parser's grammar.json."
                );
            }
            panic!(
                "grammar version skew: node kind {id}: expected {name:?} (named={named}), \
                 found {found:?} (named={found_named}). Regenerate with the parser's grammar."
            );
        }
        for &(id, name) in self.fields {
            let found = language.field_name_for_id(id);
            if found == Some(name) {
                continue;
            }
            if let Some((grammar, source, sha256)) = self.identity {
                panic!(
                    "grammar version skew: field {id}: expected {name:?}, found {found:?}. \
                     Generated for {grammar} ({source}, grammar.json SHA-256 {sha256}). \
                     Regenerate with the parser's grammar.json."
                );
            }
            panic!(
                "grammar version skew: field {id}: expected {name:?}, found {found:?}. \
                 Regenerate with the parser's grammar."
            );
        }
    }
}

/// Runner boundary result.
#[doc(hidden)]
pub enum Flow {
    Call(&'static Call),
    Return(PortId),
    Backtrack,
    Accept,
}

/// Entrypoint result boundaries.
#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct EntryBoundary {
    record: bool,
    node: bool,
}

impl EntryBoundary {
    pub const fn new(record: bool, node: bool) -> Self {
        Self { record, node }
    }

    fn begin(self, eng: &mut MatcherEngine<'_>) {
        if self.record {
            let _ = eng.emit_output_event(|_| JournalEvent::RecordOpen);
        }
    }

    fn finish(self, eng: &mut MatcherEngine<'_>) {
        if self.node {
            let _ = eng.emit_output_event(|cursor| JournalEvent::Node(cursor.node()));
            return;
        }
        if self.record {
            let _ = eng.emit_output_event(|_| JournalEvent::RecordClose);
        }
    }
}

/// Runs generated blocks across calls, returns, and backtracking.
#[doc(hidden)]
pub struct Matcher {
    pub runner: Runner,
}

impl Matcher {
    pub fn run<'a, 't>(
        &self,
        eng: &'a mut MatcherEngine<'t>,
        source: &str,
        entry: u32,
        boundary: EntryBoundary,
    ) -> Result<Option<&'a MatchJournal<'t>>, LimitExceeded> {
        boundary.begin(eng);
        let mut runner = self.runner;
        let mut block = entry;
        loop {
            match runner(eng, source, block) {
                Flow::Call(call) => {
                    eng.enter_frame(call)?;
                    runner = call.runner;
                    block = call.entry;
                }
                Flow::Accept => {
                    boundary.finish(eng);
                    return Ok(Some(eng.accepted_journal()));
                }
                Flow::Return(port) => {
                    if eng.frames_empty() {
                        assert_eq!(
                            port,
                            PortId::ZERO,
                            "entrypoint returned port {}; expected port 0",
                            port.to_byte()
                        );
                        boundary.finish(eng);
                        return Ok(Some(eng.accepted_journal()));
                    }
                    let call = eng.exit_frame();
                    block = call.returns[port.index()];
                    runner = eng
                        .current_call()
                        .map_or(self.runner, |parent| parent.runner);
                }
                Flow::Backtrack => {
                    let Some((cp, snapshot)) = eng.pop_checkpoint() else {
                        return Ok(None);
                    };
                    // Backtracking can restore a callee that already returned.
                    eng.restore_checkpoint_state(cp.state, snapshot);
                    runner = eng.current_call().map_or(self.runner, |call| call.runner);
                    block = cp.resume;
                }
            }
        }
    }
}
