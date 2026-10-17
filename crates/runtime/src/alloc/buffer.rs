use std::alloc::Layout;
use std::marker::PhantomData;
use std::ptr::NonNull;

use super::raw;
use super::{Account, Meter};
use crate::LimitExceeded;

/// Fallible, accounted growable array.
///
/// Growth admits the new capacity before allocating, so the old and new
/// blocks are charged together until the copy completes. A failed growth
/// leaves the buffer unchanged. Truncation keeps capacity, so rollback is
/// O(1) for `Copy` elements and reuses already-charged storage.
pub struct Buffer<T> {
    ptr: NonNull<T>,
    len: usize,
    cap: usize,
    account: Option<Account>,
    _owns: PhantomData<T>,
}

// SAFETY: the buffer exclusively owns its elements.
unsafe impl<T: Send> Send for Buffer<T> {}
// SAFETY: shared access only reads elements.
unsafe impl<T: Sync> Sync for Buffer<T> {}

impl<T> Buffer<T> {
    const ELEMENT_SIZE: usize = size_of::<T>();

    /// Zero-sized elements need no storage, so their capacity is unbounded.
    const INITIAL_CAP: usize = if Self::ELEMENT_SIZE == 0 {
        usize::MAX
    } else {
        0
    };

    pub const fn new() -> Self {
        Self {
            ptr: NonNull::dangling(),
            len: 0,
            cap: Self::INITIAL_CAP,
            account: None,
            _owns: PhantomData,
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Bytes of capacity this buffer holds charged.
    pub fn capacity_bytes(&self) -> u64 {
        if Self::ELEMENT_SIZE == 0 {
            return 0;
        }
        (self.cap * Self::ELEMENT_SIZE) as u64
    }

    #[inline]
    pub fn as_slice(&self) -> &[T] {
        // SAFETY: the first `len` elements are initialized.
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }

    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        // SAFETY: the first `len` elements are initialized and exclusively borrowed.
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }

    #[inline]
    pub fn last(&self) -> Option<&T> {
        self.as_slice().last()
    }

    #[inline]
    pub fn last_mut(&mut self) -> Option<&mut T> {
        self.as_mut_slice().last_mut()
    }

    #[inline]
    pub fn push(&mut self, meter: &mut Meter<'_>, value: T) -> Result<(), LimitExceeded> {
        meter.charge_bytes(Self::ELEMENT_SIZE as u64)?;
        if self.len == self.cap {
            self.grow(meter, 1)?;
        }
        // SAFETY: `len < cap` after growth.
        unsafe { self.ptr.as_ptr().add(self.len).write(value) };
        self.len += 1;
        Ok(())
    }

    #[inline]
    pub fn pop(&mut self) -> Option<T> {
        self.len = self.len.checked_sub(1)?;
        // SAFETY: the element at the old last index is initialized.
        Some(unsafe { self.ptr.as_ptr().add(self.len).read() })
    }

    /// Ensure room for `additional` more elements.
    pub fn reserve(
        &mut self,
        meter: &mut Meter<'_>,
        additional: usize,
    ) -> Result<(), LimitExceeded> {
        if self.cap - self.len >= additional {
            return Ok(());
        }
        self.grow(meter, additional)
    }

    /// Drop elements past `len`, keeping capacity.
    #[inline]
    pub fn truncate(&mut self, len: usize) {
        assert!(
            len <= self.len,
            "buffer truncation to {len} exceeds the current length {}",
            self.len
        );
        let tail = std::ptr::slice_from_raw_parts_mut(
            // SAFETY: `len <= self.len`, so the offset stays in the block.
            unsafe { self.ptr.as_ptr().add(len) },
            self.len - len,
        );
        self.len = len;
        // SAFETY: the tail elements are initialized and no longer reachable.
        unsafe { tail.drop_in_place() };
    }

    #[inline]
    pub fn clear(&mut self) {
        self.truncate(0);
    }

    #[cold]
    fn grow(&mut self, meter: &mut Meter<'_>, additional: usize) -> Result<(), LimitExceeded> {
        let (cap, layout) = self.growth_layout(additional)?;
        let bytes = layout.size() as u64;
        let copied = (self.len * Self::ELEMENT_SIZE) as u64;

        let account = meter.account();
        if let Some(owner) = &self.account {
            assert!(
                owner.same(account),
                "a runtime buffer grew under a different operation's account"
            );
        }

        meter.charge(1)?;
        meter.charge_bytes(copied)?;
        account.reserve(bytes)?;
        let Some(ptr) = raw::allocate(layout) else {
            account.release(bytes);
            return Err(LimitExceeded::Allocation { bytes });
        };
        let ptr = ptr.cast::<T>();
        // SAFETY: both blocks hold at least `len` elements and are distinct.
        unsafe {
            ptr.as_ptr()
                .copy_from_nonoverlapping(self.ptr.as_ptr(), self.len)
        };

        let old = std::mem::replace(&mut self.ptr, ptr);
        let old_cap = std::mem::replace(&mut self.cap, cap);
        if old_cap != 0 {
            // SAFETY: the old block came from `allocate` with this layout.
            unsafe { raw::deallocate(old.cast(), Self::layout(old_cap)) };
            account.release((old_cap * Self::ELEMENT_SIZE) as u64);
        }
        if self.account.is_none() {
            self.account = Some(account.clone());
        }
        Ok(())
    }

    /// Additional allocation needed by a reserve, including old/new overlap.
    pub(crate) fn reservation_bytes(&self, additional: usize) -> Result<u64, LimitExceeded> {
        if self.cap - self.len >= additional {
            return Ok(0);
        }

        Ok(self.growth_layout(additional)?.1.size() as u64)
    }

    fn growth_layout(&self, additional: usize) -> Result<(usize, Layout), LimitExceeded> {
        let required = self
            .len
            .checked_add(additional)
            .ok_or(LimitExceeded::Allocation { bytes: u64::MAX })?;
        let doubled = self.cap.saturating_mul(2).max(8);
        let cap = required.max(doubled);
        let layout = Layout::array::<T>(cap).map_err(|_| LimitExceeded::Allocation {
            bytes: (cap as u64).saturating_mul(Self::ELEMENT_SIZE as u64),
        })?;
        Ok((cap, layout))
    }

    fn layout(cap: usize) -> Layout {
        Layout::array::<T>(cap).expect("an allocated capacity has a valid layout")
    }
}

impl<T: Copy> Buffer<T> {
    /// Append a copy of `values`.
    pub fn extend_from_slice(
        &mut self,
        meter: &mut Meter<'_>,
        values: &[T],
    ) -> Result<(), LimitExceeded> {
        meter.charge_bytes(size_of_val(values) as u64)?;
        self.reserve(meter, values.len())?;
        // SAFETY: capacity covers `len + values.len()`; `values` cannot alias
        // this buffer's spare capacity.
        unsafe {
            self.ptr
                .as_ptr()
                .add(self.len)
                .copy_from_nonoverlapping(values.as_ptr(), values.len());
        }
        self.len += values.len();
        Ok(())
    }

    /// Append a copy of `self[start..]`.
    pub fn extend_from_within(
        &mut self,
        meter: &mut Meter<'_>,
        start: usize,
    ) -> Result<(), LimitExceeded> {
        assert!(start <= self.len, "copy source starts past the buffer end");
        let count = self.len - start;
        meter.charge_bytes((count * Self::ELEMENT_SIZE) as u64)?;
        self.reserve(meter, count)?;
        // SAFETY: the source range is initialized, the destination is spare
        // capacity after it, so the ranges do not overlap.
        unsafe {
            let base = self.ptr.as_ptr();
            base.add(self.len)
                .copy_from_nonoverlapping(base.add(start), count);
        }
        self.len += count;
        Ok(())
    }
}

impl<T> Default for Buffer<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "serde")]
impl<T: serde::Serialize> serde::Serialize for Buffer<T> {
    fn serialize<S: serde::Serializer>(&self, out: S) -> Result<S::Ok, S::Error> {
        self.as_slice().serialize(out)
    }
}

impl<T> std::ops::Deref for Buffer<T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Buffer<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.as_slice().fmt(formatter)
    }
}

impl<T> Drop for Buffer<T> {
    fn drop(&mut self) {
        self.clear();
        if self.cap == 0 || Self::ELEMENT_SIZE == 0 {
            return;
        }
        // SAFETY: the block came from `allocate` with this layout.
        unsafe { raw::deallocate(self.ptr.cast(), Self::layout(self.cap)) };
        let account = self
            .account
            .take()
            .expect("an allocated buffer records its account");
        account.release(self.capacity_bytes());
    }
}

impl<T> std::ops::Index<usize> for Buffer<T> {
    type Output = T;

    #[inline]
    fn index(&self, index: usize) -> &T {
        &self.as_slice()[index]
    }
}

impl<T> std::ops::IndexMut<usize> for Buffer<T> {
    #[inline]
    fn index_mut(&mut self, index: usize) -> &mut T {
        &mut self.as_mut_slice()[index]
    }
}
