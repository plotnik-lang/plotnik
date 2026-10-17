use std::alloc::Layout;
use std::fmt;
use std::iter::FusedIterator;
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::ops::Range;
use std::ptr::NonNull;

use super::family::{Publish, ResultFamily};
use super::region::Region;
use crate::LimitExceeded;
use crate::alloc::{Account, Meter, raw};
use crate::json::{self, JsonError, Style};

/// One match's region-owned result graph.
///
/// The root value is stored inline; its descendant rows and list elements
/// live in the match's region, so a `Match` can move freely and outlives the
/// operation that produced it. Read it through [`Match::view`].
pub struct Match<E: ResultFamily> {
    // Moving an owner into a function must not protect its internal references
    // against the deallocation performed by its region's destructor.
    root: MaybeUninit<E>,
    region: Region,
}

impl<E: ResultFamily> Match<E> {
    pub(crate) fn new(root: E, region: Region) -> Self {
        Self {
            root: MaybeUninit::new(root),
            region,
        }
    }

    /// The result, borrowing this match's storage.
    pub fn view<'a>(&'a self) -> &'a E::Borrowed<'a> {
        // SAFETY: `Borrowed<'a>` is `E` with its result-storage lifetime
        // shortened to `'a`, which this borrow of the storage owner bounds.
        unsafe { &*self.root.as_ptr().cast::<E::Borrowed<'a>>() }
    }

    /// Render the result as JSON, charging the producing operation's fuel.
    /// `source` must be the text the tree was parsed from.
    pub fn write_json<'t, 's>(
        &self,
        source: &str,
        style: Style,
        sink: &mut dyn fmt::Write,
    ) -> Result<(), JsonError>
    where
        E: Publish<'t, 's>,
    {
        let value = E::json(self.view());
        json::write_value(self.region.account(), value, source, style, sink)
    }
}

/// The region-owned result graphs of `find_all`, in match order.
///
/// Iterate a borrow to read them: `for q in &matches { ... }`.
pub struct Matches<E: ResultFamily> {
    table: Table,
    _roots: PhantomData<E>,
}

impl<E: ResultFamily> Matches<E> {
    pub(crate) fn new() -> Self {
        Self {
            table: Table::new(Layout::new::<E>()),
            _roots: PhantomData,
        }
    }

    /// Take ownership of one more match. On failure the match is released.
    pub(crate) fn push(
        &mut self,
        meter: &mut Meter<'_>,
        item: Match<E>,
    ) -> Result<(), LimitExceeded> {
        meter.charge_bytes(size_of::<E>() as u64)?;
        self.table.reserve(meter)?;
        // SAFETY: the table's root layout is exactly `E`; this slot is spare
        // capacity. No borrow of a root exists during mutation.
        unsafe {
            let slot = self.table.slot(self.table.len);
            slot.cast::<E>()
                .copy_from_nonoverlapping(item.root.as_ptr(), 1);
            slot.add(self.table.region_offset)
                .cast::<Region>()
                .write(item.region);
        }
        self.table.len += 1;
        Ok(())
    }

    fn get<'a>(&'a self, index: usize) -> &'a E::Borrowed<'a> {
        assert!(index < self.table.len, "match index is initialized");
        // SAFETY: the table stores initialized `E` roots, and owns all their
        // regions. Rebinding only shortens the internal storage lifetime.
        unsafe { &*self.table.slot(index).cast::<E::Borrowed<'a>>() }
    }
}

impl<'a, E: ResultFamily> IntoIterator for &'a Matches<E> {
    type Item = &'a E::Borrowed<'a>;
    type IntoIter = Iter<'a, E>;

    fn into_iter(self) -> Iter<'a, E> {
        Iter {
            matches: self,
            indices: 0..self.table.len,
        }
    }
}

pub struct Iter<'a, E: ResultFamily> {
    matches: &'a Matches<E>,
    indices: Range<usize>,
}

impl<'a, E: ResultFamily> Iterator for Iter<'a, E> {
    type Item = &'a E::Borrowed<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        Some(self.matches.get(self.indices.next()?))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.indices.size_hint()
    }
}

impl<E: ResultFamily> ExactSizeIterator for Iter<'_, E> {}

impl<E: ResultFamily> FusedIterator for Iter<'_, E> {}

impl<'a, E: ResultFamily> DoubleEndedIterator for Iter<'a, E> {
    fn next_back(&mut self) -> Option<Self::Item> {
        Some(self.matches.get(self.indices.next_back()?))
    }
}

/// The table owns bytes and region descriptors, never a generic result value.
/// Its destructor must not require the result's external borrows to be live.
struct Table {
    ptr: NonNull<u8>,
    root: Layout,
    stride: usize,
    align: usize,
    region_offset: usize,
    len: usize,
    cap: usize,
    account: Option<Account>,
}

// SAFETY: mutation requires exclusive access, and region release is thread-safe.
unsafe impl Send for Table {}
// SAFETY: shared access only reads immutable roots through `Matches<E>`.
unsafe impl Sync for Table {}

impl Table {
    fn new(root: Layout) -> Self {
        Self {
            ptr: NonNull::dangling(),
            root,
            stride: 0,
            align: 1,
            region_offset: 0,
            len: 0,
            cap: 0,
            account: None,
        }
    }

    fn reserve(&mut self, meter: &mut Meter<'_>) -> Result<(), LimitExceeded> {
        if self.len < self.cap {
            return Ok(());
        }

        let overflow = LimitExceeded::Allocation { bytes: u64::MAX };
        let (slot, region_offset) = self
            .root
            .extend(Layout::new::<Region>())
            .map_err(|_| overflow)?;
        let slot = slot.pad_to_align();
        let cap = self.cap.checked_mul(2).ok_or(overflow)?.max(8);
        let bytes = slot.size().checked_mul(cap).ok_or(overflow)?;
        let layout = Layout::from_size_align(bytes, slot.align()).map_err(|_| overflow)?;
        let copied = self.len * slot.size();
        let account = meter.account();
        if let Some(owner) = &self.account {
            assert!(owner.same(account), "match table belongs to this operation");
        }

        meter.charge(1)?;
        meter.charge_bytes(copied as u64)?;
        account.reserve(bytes as u64)?;
        let Some(ptr) = raw::allocate(layout) else {
            account.release(bytes as u64);
            return Err(LimitExceeded::Allocation {
                bytes: bytes as u64,
            });
        };
        // SAFETY: the new block has room for every initialized slot. Copying
        // transfers regions, so the old block is freed without dropping them.
        unsafe {
            ptr.as_ptr()
                .copy_from_nonoverlapping(self.ptr.as_ptr(), copied)
        };
        if self.cap != 0 {
            let old = self.layout();
            // SAFETY: the table exclusively owns the old allocation.
            unsafe { raw::deallocate(self.ptr, old) };
            account.release(old.size() as u64);
        }
        self.ptr = ptr;
        self.stride = slot.size();
        self.align = slot.align();
        self.region_offset = region_offset;
        self.cap = cap;
        if self.account.is_none() {
            self.account = Some(account.clone());
        }
        Ok(())
    }

    fn layout(&self) -> Layout {
        Layout::from_size_align(self.cap * self.stride, self.align)
            .expect("an allocated table has a valid layout")
    }

    unsafe fn slot(&self, index: usize) -> *mut u8 {
        // SAFETY: callers restrict indices to the allocated capacity.
        unsafe { self.ptr.as_ptr().add(index * self.stride) }
    }
}

impl Drop for Table {
    fn drop(&mut self) {
        if self.cap == 0 {
            return;
        }

        for index in 0..self.len {
            // SAFETY: every committed slot contains one initialized region.
            // Roots are non-dropping and are never read during destruction.
            unsafe {
                self.slot(index)
                    .add(self.region_offset)
                    .cast::<Region>()
                    .drop_in_place()
            };
        }
        let layout = self.layout();
        // SAFETY: all regions are released and no root borrows remain.
        unsafe { raw::deallocate(self.ptr, layout) };
        self.account
            .as_ref()
            .expect("allocated table has an account")
            .release(layout.size() as u64);
    }
}

#[cfg(test)]
#[path = "owner_tests.rs"]
mod owner_tests;
