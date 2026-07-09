//! The typed replay's cursor over a committed effect log.
//!
//! A generated matcher commits the same effect stream the VM commits; the
//! generated per-type readers then replay that stream once, on the winning
//! path, into the query's typed output. [`TraceReader`] is the cursor those
//! readers share: it only knows the stream's vocabulary, while the readers
//! carry the schema (which entries are possible where — proven at emit by the
//! same analysis the bytecode effect-stack validation checks).
//!
//! Every miss here is emitter/reader drift, not anything an input can cause,
//! so misses panic with the position and the offending entry.

use tree_sitter::Node;

use crate::{EffectLog, RuntimeEffect};

/// `set_index` sentinel: no `Set` closes a value starting at this position.
const NO_SET: u32 = u32::MAX;

pub struct TraceReader<'a, 't> {
    entries: &'a [RuntimeEffect<'t>],
    pos: usize,
    /// For each position, where the `Set` that closes a field value starting
    /// there sits — [`Self::peek_set`]'s answer, precomputed. One backward
    /// pass at construction keeps replay linear; peeking on demand would
    /// rescan every nested composite once per enclosing scope, going
    /// quadratic on deep recursive values.
    set_index: Vec<u32>,
}

impl<'a, 't> TraceReader<'a, 't> {
    pub fn new(log: &'a EffectLog<'t>) -> Self {
        let entries = log.as_slice();
        Self {
            entries,
            pos: 0,
            set_index: build_set_index(entries),
        }
    }

    /// Assert the whole trace was consumed — the entrypoint's value is the
    /// entire committed stream, so leftovers mean the reader lost sync.
    pub fn finish(self) {
        assert!(
            self.pos == self.entries.len(),
            "trace reader: {} of {} entries left unread after the value",
            self.entries.len() - self.pos,
            self.entries.len(),
        );
    }

    fn next(&mut self) -> &'a RuntimeEffect<'t> {
        let entry = self
            .entries
            .get(self.pos)
            .expect("trace reader: read past the end of the committed trace");
        self.pos += 1;
        entry
    }

    fn peek(&self) -> Option<&'a RuntimeEffect<'t>> {
        self.entries.get(self.pos)
    }

    /// The member index of the `Set` that will close the field value starting
    /// at the cursor. Struct scopes need it because the stream is
    /// value-first: the entries of a field's value arrive *before* the `Set`
    /// that names the field, and sibling fields of one struct can differ in
    /// type — so the reader peeks ahead to pick the right typed reader, then
    /// consumes the value linearly. The answer — the first `Set` past the
    /// cursor's balanced composite values — comes from the precomputed
    /// [`Self::set_index`].
    pub fn peek_set(&self) -> u16 {
        let set_pos = *self
            .set_index
            .get(self.pos)
            .expect("trace reader: peeked past the end of the committed trace");
        assert!(
            set_pos != NO_SET,
            "trace reader: no Set closes the field value at {}",
            self.pos
        );
        match &self.entries[set_pos as usize] {
            RuntimeEffect::Set(index) => *index,
            other => unreachable!("set_index addresses Set entries, found {other:?}"),
        }
    }

    /// Consume a `Null` if it is next. How optional values read: `Null` is the
    /// whole absent value, anything else is the present value.
    pub fn take_null(&mut self) -> bool {
        if matches!(self.peek(), Some(RuntimeEffect::Null)) {
            self.pos += 1;
            return true;
        }
        false
    }

    pub fn at_array_close(&self) -> bool {
        matches!(self.peek(), Some(RuntimeEffect::ArrayClose))
    }

    pub fn at_struct_close(&self) -> bool {
        matches!(self.peek(), Some(RuntimeEffect::StructClose))
    }

    pub fn at_enum_close(&self) -> bool {
        matches!(self.peek(), Some(RuntimeEffect::EnumClose))
    }

    pub fn expect_node(&mut self) -> Node<'t> {
        match self.next() {
            RuntimeEffect::Node(node) => *node,
            other => self.mismatch("Node", other),
        }
    }

    pub fn expect_set(&mut self) -> u16 {
        match self.next() {
            RuntimeEffect::Set(index) => *index,
            other => self.mismatch("Set", other),
        }
    }

    pub fn expect_enum_open(&mut self) -> u16 {
        match self.next() {
            RuntimeEffect::EnumOpen(index) => *index,
            other => self.mismatch("EnumOpen", other),
        }
    }

    pub fn expect_array_open(&mut self) {
        match self.next() {
            RuntimeEffect::ArrayOpen => {}
            other => self.mismatch("ArrayOpen", other),
        }
    }

    pub fn expect_array_close(&mut self) {
        match self.next() {
            RuntimeEffect::ArrayClose => {}
            other => self.mismatch("ArrayClose", other),
        }
    }

    pub fn expect_push(&mut self) {
        match self.next() {
            RuntimeEffect::Push => {}
            other => self.mismatch("Push", other),
        }
    }

    pub fn expect_struct_open(&mut self) {
        match self.next() {
            RuntimeEffect::StructOpen => {}
            other => self.mismatch("StructOpen", other),
        }
    }

    pub fn expect_struct_close(&mut self) {
        match self.next() {
            RuntimeEffect::StructClose => {}
            other => self.mismatch("StructClose", other),
        }
    }

    pub fn expect_enum_close(&mut self) {
        match self.next() {
            RuntimeEffect::EnumClose => {}
            other => self.mismatch("EnumClose", other),
        }
    }

    fn mismatch(&self, expected: &str, found: &RuntimeEffect<'_>) -> ! {
        panic!(
            "trace reader: expected {expected} at {}, found {found:?}",
            self.pos - 1,
        )
    }
}

/// For each position, the position of the first `Set` past the balanced
/// composite values starting there — the `Set` that closes a field value
/// beginning at that entry, or [`NO_SET`] where none does (positions no
/// reader ever peeks at: closes, and value starts whose `Set` lives in an
/// enclosing scope).
///
/// One backward pass. `cur` is the answer for the level being scanned;
/// meeting a `*Close` right-to-left *enters* that group, so the outer level's
/// answer is parked on `outer` and `cur` restarts; meeting the matching
/// `*Open` leaves the group, and the parked answer — the first `Set` after
/// the group — is exactly the answer *at* the open (a composite field value
/// starts there) and for whatever precedes it on the outer level.
fn build_set_index(entries: &[RuntimeEffect<'_>]) -> Vec<u32> {
    assert!(
        u32::try_from(entries.len()).is_ok_and(|len| len < NO_SET),
        "trace length fits the u32 index space"
    );
    let mut index = vec![NO_SET; entries.len()];
    let mut cur = NO_SET;
    let mut outer: Vec<u32> = Vec::new();
    for (i, entry) in entries.iter().enumerate().rev() {
        match entry {
            RuntimeEffect::Set(_) => {
                cur = i as u32;
                index[i] = cur;
            }
            RuntimeEffect::ArrayClose | RuntimeEffect::StructClose | RuntimeEffect::EnumClose => {
                outer.push(cur);
                cur = NO_SET;
            }
            RuntimeEffect::ArrayOpen | RuntimeEffect::StructOpen | RuntimeEffect::EnumOpen(_) => {
                cur = outer
                    .pop()
                    .expect("open/close balance proven by the effect-stack validation");
                index[i] = cur;
            }
            _ => index[i] = cur,
        }
    }
    index
}
