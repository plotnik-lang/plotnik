pub(crate) mod checked;

use std::num::NonZeroU64;

use tree_sitter::Node;

use crate::alloc::{Buffer, Meter};
use crate::{LimitExceeded, Nav, NodeClass, NodeFieldId, SkipClass, SkipPolicy};
use checked::CheckedCursor;

const SNAPSHOT_CAP: usize = 64;
const SNAPSHOT_ACTIVATION_WIDE_MISSES: u32 = 32;
const SNAPSHOT_ACTIVATION_MIN_JUMP: u32 = 32;

struct Snapshot<'t> {
    seq: u64,
    refs: u32,
    cursor: CheckedCursor<'t>,
}

/// A checked cursor rooted at the application node. All paths, including
/// cached snapshots and field-test scratch, retain exact admitted capacities.
pub struct CursorWrapper<'t> {
    cursor: CheckedCursor<'t>,
    scratch: CheckedCursor<'t>,
    snapshots: Buffer<Snapshot<'t>>,
    next_seq: u64,
    wide_restore_misses: u32,
}

impl<'t> CursorWrapper<'t> {
    pub fn new(node: Node<'t>) -> Self {
        Self {
            cursor: CheckedCursor::new(node),
            scratch: CheckedCursor::new(node),
            snapshots: Buffer::new(),
            next_seq: 0,
            wide_restore_misses: 0,
        }
    }

    pub(crate) fn reset(&mut self, node: Node<'t>) {
        for snapshot in self.snapshots.as_mut_slice() {
            snapshot.refs = 0;
            snapshot.seq = 0;
        }
        self.next_seq = 0;
        self.wide_restore_misses = 0;
        self.cursor.reset(node);
        self.scratch.reset(node);
    }

    pub fn node(&self) -> Node<'t> {
        self.cursor.node()
    }
    pub fn descendant_index(&self) -> u32 {
        self.cursor.index()
    }

    pub fn goto_descendant(
        &mut self,
        meter: &mut Meter<'_>,
        index: u32,
    ) -> Result<(), LimitExceeded> {
        self.cursor.seek(meter, index)
    }

    pub fn snapshot(
        &mut self,
        meter: &mut Meter<'_>,
        refs: u32,
    ) -> Result<Option<NonZeroU64>, LimitExceeded> {
        if !self.snapshots_active() {
            return Ok(None);
        }
        assert!(refs > 0, "snapshot has checkpoint references");
        let seq = self
            .next_seq
            .checked_add(1)
            .ok_or(LimitExceeded::Allocation { bytes: u64::MAX })?;
        let index = (seq - 1) as usize % SNAPSHOT_CAP;
        meter.charge(self.snapshots.len() as u64)?;
        let cache = self.snapshots.capacity_bytes()
            + self
                .snapshots
                .iter()
                .map(|slot| slot.cursor.capacity_bytes())
                .sum::<u64>();
        let destination = self
            .snapshots
            .get(index)
            .map_or(0, |slot| slot.cursor.capacity_bytes());
        let growth = 2 * self.cursor.capacity_bytes().max(destination);
        let metadata = if index == self.snapshots.len() {
            self.snapshots.reservation_bytes(1)?
        } else {
            0
        };
        if cache.saturating_add(growth).saturating_add(metadata) > 96 * 1024 {
            return Ok(None);
        }

        let admission = (|| {
            if index == self.snapshots.len() {
                self.snapshots.push(
                    meter,
                    Snapshot {
                        seq: 0,
                        refs: 0,
                        cursor: CheckedCursor::new(self.node()),
                    },
                )?;
            }
            let snapshot = &mut self.snapshots[index];
            snapshot.seq = 0;
            snapshot.refs = 0;
            snapshot.cursor.copy_from(meter, &self.cursor)?;
            snapshot.seq = seq;
            snapshot.refs = refs;
            self.next_seq = seq;
            Ok(NonZeroU64::new(seq))
        })();
        match admission {
            Err(LimitExceeded::Memory { .. } | LimitExceeded::Allocation { .. }) => Ok(None),
            result => result,
        }
    }

    pub fn restore(
        &mut self,
        meter: &mut Meter<'_>,
        snapshot: Option<NonZeroU64>,
        index: u32,
    ) -> Result<bool, LimitExceeded> {
        if let Some(seq) = snapshot {
            let slot = (seq.get() - 1) as usize % SNAPSHOT_CAP;
            if let Some(snapshot) = self.snapshots.as_mut_slice().get_mut(slot)
                && snapshot.seq == seq.get()
            {
                snapshot.refs = snapshot
                    .refs
                    .checked_sub(1)
                    .expect("snapshot reference is live");
                if self.cursor.index() != index {
                    match self.cursor.copy_from(meter, &snapshot.cursor) {
                        Err(LimitExceeded::Memory { .. } | LimitExceeded::Allocation { .. }) => {
                            self.cursor.seek(meter, index)?
                        }
                        result => result?,
                    }
                }
                assert_eq!(
                    self.cursor.index(),
                    index,
                    "snapshot restores its checkpoint"
                );
                return Ok(false);
            }
        }
        self.restore_without_snapshot(meter, index)
    }

    pub fn restore_without_snapshot(
        &mut self,
        meter: &mut Meter<'_>,
        index: u32,
    ) -> Result<bool, LimitExceeded> {
        let current = self.descendant_index();
        if current == index {
            return Ok(false);
        }
        let was_active = self.snapshots_active();
        if current.abs_diff(index) >= SNAPSHOT_ACTIVATION_MIN_JUMP && !was_active {
            self.wide_restore_misses += 1;
        }
        self.cursor.seek(meter, index)?;
        assert_eq!(
            self.descendant_index(),
            index,
            "checked seek restores its checkpoint"
        );
        Ok(!was_active && self.snapshots_active())
    }

    pub fn snapshots_active(&self) -> bool {
        self.wide_restore_misses >= SNAPSHOT_ACTIVATION_WIDE_MISSES
    }

    pub fn has_field(
        &mut self,
        meter: &mut Meter<'_>,
        field: NodeFieldId,
    ) -> Result<bool, LimitExceeded> {
        self.cursor.has_field(meter, u16::from(field))
    }

    pub fn field_absent(
        &mut self,
        meter: &mut Meter<'_>,
        field: NodeFieldId,
    ) -> Result<bool, LimitExceeded> {
        self.scratch.copy_from(meter, &self.cursor)?;
        if !self.scratch.first_child(meter)? {
            return Ok(true);
        }
        loop {
            if self.scratch.has_field(meter, u16::from(field))? {
                return Ok(false);
            }
            if !self.scratch.next_sibling(meter)? {
                return Ok(true);
            }
        }
    }

    fn class(&mut self, meter: &mut Meter<'_>) -> Result<NodeClass, LimitExceeded> {
        Ok(NodeClass {
            anonymous: !self.node().is_named(),
            extra: self.node().is_extra() || self.cursor.path_extra(meter)?,
        })
    }

    pub fn navigate(
        &mut self,
        meter: &mut Meter<'_>,
        nav: Nav,
    ) -> Result<Option<SkipPolicy>, LimitExceeded> {
        let moved = match nav {
            Nav::Epsilon => unreachable!("epsilon transitions bypass navigation"),
            Nav::Stay | Nav::StayExact => true,
            Nav::Down | Nav::DownSkip | Nav::DownSkipExtras | Nav::DownExact => {
                self.cursor.first_child(meter)?
            }
            Nav::Next | Nav::NextSkip | Nav::NextSkipExtras | Nav::NextExact => {
                self.cursor.next_sibling(meter)?
            }
            Nav::ChildlessSkipTrivia => self.childless(meter, SkipClass::Trivia)?,
            Nav::ChildlessSkipExtras => self.childless(meter, SkipClass::Extras)?,
            Nav::ChildlessExact => self.childless(meter, SkipClass::Exact)?,
            Nav::Up(n) => self.up(meter, n, SkipClass::Any)?,
            Nav::UpSkipTrivia(n) => self.up(meter, n, SkipClass::Trivia)?,
            Nav::UpSkipExtras(n) => self.up(meter, n, SkipClass::Extras)?,
            Nav::UpExact(n) => self.up(meter, n, SkipClass::Exact)?,
        };
        Ok(moved.then_some(nav.skip_policy()))
    }

    fn childless(&mut self, meter: &mut Meter<'_>, skip: SkipClass) -> Result<bool, LimitExceeded> {
        let origin = self.descendant_index();
        if !self.cursor.first_child(meter)? {
            return Ok(true);
        }
        let holds = self.remaining_skippable(meter, skip)?;
        self.cursor.seek(meter, origin)?;
        Ok(holds)
    }

    fn remaining_skippable(
        &mut self,
        meter: &mut Meter<'_>,
        skip: SkipClass,
    ) -> Result<bool, LimitExceeded> {
        loop {
            if !skip.admits(self.class(meter)?) {
                return Ok(false);
            }
            if !self.cursor.next_sibling(meter)? {
                return Ok(true);
            }
        }
    }

    fn up(
        &mut self,
        meter: &mut Meter<'_>,
        levels: u8,
        skip: SkipClass,
    ) -> Result<bool, LimitExceeded> {
        let origin = self.descendant_index();
        for _ in 0..levels {
            if skip != SkipClass::Any {
                let saved = self.descendant_index();
                let holds =
                    !self.cursor.next_sibling(meter)? || self.remaining_skippable(meter, skip)?;
                self.cursor.seek(meter, saved)?;
                if !holds {
                    self.cursor.seek(meter, origin)?;
                    return Ok(false);
                }
            }
            if !self.cursor.parent(meter)? {
                self.cursor.seek(meter, origin)?;
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub fn continue_search(
        &mut self,
        meter: &mut Meter<'_>,
        policy: SkipPolicy,
    ) -> Result<bool, LimitExceeded> {
        let skip = match policy {
            SkipPolicy::Exact => return Ok(false),
            SkipPolicy::Trivia => SkipClass::Trivia,
            SkipPolicy::Extras => SkipClass::Extras,
            SkipPolicy::Any => SkipClass::Any,
        };
        if !skip.admits(self.class(meter)?) {
            return Ok(false);
        }
        self.cursor.next_sibling(meter)
    }
}
