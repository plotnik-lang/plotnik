//! The typed decoder's cursor over a committed match journal.
//!
//! A generated matcher commits the same journal the VM commits; the generated
//! per-type decoders then decode that journal once, on the winning
//! path, into the query's typed output. [`ResultDecoder`] is the cursor those
//! decoders share: it only knows the stream's vocabulary, while the decoders
//! carry the schema (which entries are possible where — proven at emit by the
//! same analysis the bytecode effect-stack validation checks).
//!
//! Every miss here is emitter/decoder drift, not anything an input can cause,
//! so misses panic with the position and the offending entry.

use tree_sitter::Node;

use crate::{JournalEvent, OutputEvents, node_text, source_text};

/// `record_set_index` sentinel: no `RecordSet` closes a value starting here.
const NO_RECORD_SET: u32 = u32::MAX;

pub struct ResultDecoder<'a, 't, 's> {
    events: OutputEvents<'a, 't>,
    source: &'s str,
    pos: usize,
    /// For each position, where the `RecordSet` that closes a field value
    /// starting there sits — [`Self::peek_record_set`]'s answer, precomputed. One backward
    /// pass at construction keeps decoding linear; peeking on demand would
    /// rescan every nested composite once per enclosing scope, going
    /// quadratic on deep recursive values.
    record_set_index: Vec<u32>,
}

impl<'a, 't, 's> ResultDecoder<'a, 't, 's> {
    pub fn new(events: OutputEvents<'a, 't>, source: &'s str) -> Self {
        Self {
            record_set_index: build_record_set_index(&events),
            events,
            source,
            pos: 0,
        }
    }

    /// Assert the whole output-event stream was consumed — the entry point's
    /// value is the entire stream, so leftovers mean the decoder lost sync.
    pub fn finish(self) {
        assert!(
            self.pos == self.events.len(),
            "result decoder: {} of {} events left unread after the value",
            self.events.len() - self.pos,
            self.events.len(),
        );
    }

    fn next(&mut self) -> &'a JournalEvent<'t> {
        let entry = self
            .events
            .get(self.pos)
            .expect("result decoder: read past the end of the committed journal");
        self.pos += 1;
        entry
    }

    fn peek(&self) -> Option<&'a JournalEvent<'t>> {
        self.events.get(self.pos)
    }

    /// The member index of the `RecordSet` that will close the field value
    /// starting at the cursor. Record scopes need it because the stream is
    /// value-first: the entries of a field's value arrive *before* the
    /// `RecordSet` that names the field, and sibling fields of one record can differ in
    /// type — so the decoder peeks ahead to pick the right nested decoder, then
    /// consumes the value linearly. The answer — the first `RecordSet` past the
    /// cursor's balanced composite values — comes from the precomputed
    /// [`Self::record_set_index`].
    pub fn peek_record_set(&self) -> u16 {
        let record_set_pos = *self
            .record_set_index
            .get(self.pos)
            .expect("result decoder: peeked past the end of the committed journal");
        assert!(
            record_set_pos != NO_RECORD_SET,
            "result decoder: no RecordSet closes the field value at {}",
            self.pos
        );
        match &self.events[record_set_pos as usize] {
            JournalEvent::RecordSet(index) => *index,
            other => {
                unreachable!("record_set_index addresses RecordSet entries, found {other:?}")
            }
        }
    }

    /// Consume `Absent` if it is next. For options, `Absent` is the
    /// whole absent value, anything else is the present value.
    pub fn take_absent(&mut self) -> bool {
        if matches!(self.peek(), Some(JournalEvent::Absent)) {
            self.pos += 1;
            return true;
        }
        self.take_unmarked_str()
    }

    fn take_unmarked_str(&mut self) -> bool {
        if !matches!(self.peek(), Some(JournalEvent::ScalarOpen)) {
            return false;
        }

        let mut depth = 0_u32;
        let mut marked = false;
        for index in self.pos..self.events.len() {
            match &self.events[index] {
                JournalEvent::ScalarOpen => depth += 1,
                JournalEvent::ScalarMark(_) => marked = true,
                JournalEvent::StrClose => {
                    depth -= 1;
                    if depth != 0 {
                        continue;
                    }
                    if marked {
                        return false;
                    }
                    self.pos = index + 1;
                    return true;
                }
                JournalEvent::BoolClose(_) => {
                    depth -= 1;
                    if depth == 0 {
                        return false;
                    }
                }
                _ => {}
            }
        }
        unreachable!("validated output events balance every scalar frame")
    }

    pub fn at_list_close(&self) -> bool {
        matches!(self.peek(), Some(JournalEvent::ListClose))
    }

    pub fn at_record_close(&self) -> bool {
        matches!(self.peek(), Some(JournalEvent::RecordClose))
    }

    pub fn at_variant_close(&self) -> bool {
        matches!(self.peek(), Some(JournalEvent::VariantClose))
    }

    pub fn expect_node(&mut self) -> Node<'t> {
        match self.next() {
            JournalEvent::Node(node) => *node,
            other => self.mismatch("Node", other),
        }
    }

    pub fn expect_str(&mut self) -> &'s str {
        if let Some(JournalEvent::NodeStr(node)) = self.peek() {
            let value = node_text(self.source, node);
            self.pos += 1;
            return value;
        }
        let (range, close) = self.read_scalar();
        if !matches!(close, JournalEvent::StrClose) {
            self.mismatch("StrClose", close);
        }
        let range = range.expect("a non-null text decoder requires at least one scalar mark");
        source_text(self.source, range)
    }

    pub fn expect_bool(&mut self) -> bool {
        if matches!(self.peek(), Some(JournalEvent::NodeBool(_))) {
            self.pos += 1;
            return true;
        }
        if let Some(JournalEvent::BoolValue(value)) = self.peek() {
            let value = *value;
            self.pos += 1;
            return value;
        }
        let (_, close) = self.read_scalar();
        match close {
            JournalEvent::BoolClose(value) => *value,
            other => self.mismatch("BoolClose", other),
        }
    }

    fn read_scalar(&mut self) -> (Option<std::ops::Range<usize>>, &'a JournalEvent<'t>) {
        match self.next() {
            JournalEvent::ScalarOpen => {}
            other => self.mismatch("ScalarOpen", other),
        }
        let mut depth = 1_u32;
        let mut range: Option<std::ops::Range<usize>> = None;
        loop {
            let entry = self.next();
            match entry {
                JournalEvent::ScalarOpen => depth += 1,
                JournalEvent::ScalarMark(node) => {
                    let mark = node.start_byte()..node.end_byte();
                    range = Some(match range {
                        Some(current) => current.start.min(mark.start)..current.end.max(mark.end),
                        None => mark,
                    });
                }
                JournalEvent::StrClose | JournalEvent::BoolClose(_) => {
                    depth -= 1;
                    if depth == 0 {
                        return (range, entry);
                    }
                }
                _ => {}
            }
        }
    }

    pub fn expect_record_set(&mut self) -> u16 {
        match self.next() {
            JournalEvent::RecordSet(index) => *index,
            other => self.mismatch("RecordSet", other),
        }
    }

    pub fn expect_variant_open(&mut self) -> u16 {
        match self.next() {
            JournalEvent::VariantOpen(index) => *index,
            other => self.mismatch("VariantOpen", other),
        }
    }

    pub fn expect_list_open(&mut self) {
        match self.next() {
            JournalEvent::ListOpen => {}
            other => self.mismatch("ListOpen", other),
        }
    }

    pub fn expect_list_close(&mut self) {
        match self.next() {
            JournalEvent::ListClose => {}
            other => self.mismatch("ListClose", other),
        }
    }

    pub fn expect_array_push(&mut self) {
        match self.next() {
            JournalEvent::ArrayPush => {}
            other => self.mismatch("ArrayPush", other),
        }
    }

    pub fn expect_record_open(&mut self) {
        match self.next() {
            JournalEvent::RecordOpen => {}
            other => self.mismatch("RecordOpen", other),
        }
    }

    pub fn expect_record_close(&mut self) {
        match self.next() {
            JournalEvent::RecordClose => {}
            other => self.mismatch("RecordClose", other),
        }
    }

    pub fn expect_variant_close(&mut self) {
        match self.next() {
            JournalEvent::VariantClose => {}
            other => self.mismatch("VariantClose", other),
        }
    }

    fn mismatch(&self, expected: &str, found: &JournalEvent<'_>) -> ! {
        panic!(
            "result decoder: expected {expected} at {}, found {found:?}",
            self.pos - 1,
        )
    }
}

/// For each position, the position of the first `RecordSet` past the balanced
/// composite values starting there — the `RecordSet` that closes a field value
/// beginning at that entry, or [`NO_RECORD_SET`] where none does (positions no
/// decoder ever peeks at: closes, and value starts whose `RecordSet` lives in an
/// enclosing scope).
///
/// One backward pass. `cur` is the answer for the level being scanned;
/// meeting a `*Close` right-to-left *enters* that group, so the outer level's
/// answer is parked on `outer` and `cur` restarts; meeting the matching
/// `*Open` leaves the group, and the parked answer — the first `RecordSet` after
/// the group — is exactly the answer *at* the open (a composite field value
/// starts there) and for whatever precedes it on the outer level.
fn build_record_set_index(events: &OutputEvents<'_, '_>) -> Vec<u32> {
    assert!(
        u32::try_from(events.len()).is_ok_and(|len| len < NO_RECORD_SET),
        "output-event count fits the u32 index space"
    );
    let mut index = vec![NO_RECORD_SET; events.len()];
    let mut cur = NO_RECORD_SET;
    let mut outer: Vec<u32> = Vec::new();
    for (i, entry) in events.iter().enumerate().rev() {
        match entry {
            JournalEvent::RecordSet(_) => {
                cur = i as u32;
                index[i] = cur;
            }
            JournalEvent::ListClose
            | JournalEvent::RecordClose
            | JournalEvent::VariantClose
            | JournalEvent::StrClose
            | JournalEvent::BoolClose(_) => {
                outer.push(cur);
                cur = NO_RECORD_SET;
            }
            JournalEvent::ListOpen
            | JournalEvent::RecordOpen
            | JournalEvent::VariantOpen(_)
            | JournalEvent::ScalarOpen => {
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
