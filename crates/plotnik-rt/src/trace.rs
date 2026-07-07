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

pub struct TraceReader<'a, 't> {
    entries: &'a [RuntimeEffect<'t>],
    pos: usize,
}

impl<'a, 't> TraceReader<'a, 't> {
    pub fn new(log: &'a EffectLog<'t>) -> Self {
        Self {
            entries: log.as_slice(),
            pos: 0,
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
    /// consumes the value linearly. The scan skips balanced composite values;
    /// the first `Set` at depth zero is the one that owns the cursor's value.
    pub fn peek_set(&self) -> u16 {
        let mut depth = 0usize;
        for entry in &self.entries[self.pos..] {
            match entry {
                RuntimeEffect::ArrayOpen
                | RuntimeEffect::StructOpen
                | RuntimeEffect::EnumOpen(_) => depth += 1,
                RuntimeEffect::ArrayClose
                | RuntimeEffect::StructClose
                | RuntimeEffect::EnumClose => {
                    depth = depth
                        .checked_sub(1)
                        .expect("trace reader: close below the field value being peeked");
                }
                RuntimeEffect::Set(index) if depth == 0 => return *index,
                _ => {}
            }
        }
        panic!(
            "trace reader: no Set closes the field value at {}",
            self.pos
        )
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
