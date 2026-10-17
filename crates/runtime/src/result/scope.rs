use std::cell::Cell;
use std::marker::PhantomData;
use std::ptr::NonNull;

use crate::{LimitExceeded, Meter};

type Invariant<'a> = PhantomData<fn(&'a ()) -> &'a ()>;

/// Checked bump storage inside one match's result block.
///
/// Generated publishers place every referenced value and every list element
/// here. Each placement checks alignment and remaining capacity itself, so a
/// wrong size plan or a wrong publisher fails an assertion instead of writing
/// out of bounds. A slice becomes visible only after all its elements are
/// written. Values are `Copy`, so nothing in the block needs dropping.
///
/// Writes go through the block's base pointer, which keeps provenance over
/// the whole block, so no mutable reference ever overlaps a shared reference
/// handed out earlier. `'a` is invariant: a scope cannot be shortened into
/// another scope's lifetime.
pub struct Scope<'a> {
    base: NonNull<u8>,
    size: usize,
    used: Cell<usize>,
    _block: Invariant<'a>,
}

impl<'a> Scope<'a> {
    /// # Safety
    ///
    /// `base..base + size` must be writable, unaliased storage that stays
    /// allocated and unmoved for `'a`, and `'a` must not outlive it.
    pub(crate) unsafe fn new(base: NonNull<u8>, size: usize) -> Self {
        Self {
            base,
            size,
            used: Cell::new(0),
            _block: PhantomData,
        }
    }

    /// An empty scope for results that reference nothing.
    pub(crate) fn empty() -> Self {
        // SAFETY: a zero-size scope never writes.
        unsafe { Self::new(NonNull::dangling(), 0) }
    }

    /// Bytes placed so far, including alignment padding.
    pub(crate) fn used(&self) -> usize {
        self.used.get()
    }

    /// Move `value` into the block and return a reference to it.
    pub fn push<T: Copy + 'a>(
        &self,
        meter: &mut Meter<'_>,
        value: T,
    ) -> Result<&'a T, LimitExceeded> {
        meter.charge_bytes(size_of::<T>() as u64)?;
        let ptr = self.place::<T>(1);
        // SAFETY: `place` returned aligned, in-bounds, never-used storage for
        // one `T` derived from the block's base pointer.
        unsafe {
            ptr.write(value);
            Ok(&*ptr)
        }
    }

    /// Reserve `len` contiguous elements, to be written in order.
    pub fn slice<T: Copy + 'a>(&self, len: usize) -> Slice<'a, T> {
        let ptr = if len == 0 {
            NonNull::dangling().as_ptr()
        } else {
            self.place::<T>(len)
        };
        Slice {
            ptr,
            len,
            filled: 0,
            _block: PhantomData,
            _elements: PhantomData,
        }
    }

    fn place<T>(&self, count: usize) -> *mut T {
        let align = align_of::<T>();
        let bytes = size_of::<T>()
            .checked_mul(count)
            .expect("a placement size fits usize");
        if bytes == 0 {
            return NonNull::dangling().as_ptr();
        }

        let address = self.base.as_ptr() as usize + self.used.get();
        let padding = address.next_multiple_of(align) - address;
        let offset = self.used.get() + padding;
        let end = offset
            .checked_add(bytes)
            .expect("a placement end fits usize");
        assert!(
            end <= self.size,
            "result placement of {bytes} bytes at offset {offset} overruns the {}-byte block",
            self.size
        );
        self.used.set(end);
        // SAFETY: `offset..end` lies inside the block.
        unsafe { self.base.as_ptr().add(offset).cast::<T>() }
    }
}

/// A reserved run of list elements in a [`Scope`].
///
/// Not `Copy`: exactly one writer fills each reservation, in order, and
/// [`Slice::finish`] consumes it.
pub struct Slice<'a, T> {
    ptr: *mut T,
    len: usize,
    filled: usize,
    _block: Invariant<'a>,
    _elements: PhantomData<T>,
}

impl<'a, T: Copy + 'a> Slice<'a, T> {
    /// Write the next element.
    pub fn write(&mut self, meter: &mut Meter<'_>, value: T) -> Result<(), LimitExceeded> {
        assert!(
            self.filled < self.len,
            "list writer overran its {} reserved elements",
            self.len
        );
        meter.charge_bytes(size_of::<T>() as u64)?;
        // SAFETY: element `filled` lies inside this reservation, which no
        // reference covers until `finish`.
        unsafe { self.ptr.add(self.filled).write(value) };
        self.filled += 1;
        Ok(())
    }

    /// Expose the elements once every one of them is written.
    pub fn finish(self) -> &'a [T] {
        assert_eq!(
            self.filled, self.len,
            "list writer finished after {} of {} elements",
            self.filled, self.len
        );
        // SAFETY: all `len` elements are initialized, and nothing writes to
        // them again: this writer is consumed.
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }
}

/// Bump-allocation plan matching [`Scope`]'s placement order.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Plan {
    size: usize,
    align: usize,
}

impl Plan {
    pub(crate) fn new() -> Self {
        Self { size: 0, align: 1 }
    }

    /// Account one placement of `count` values of `layout`.
    pub(crate) fn place(&mut self, layout: std::alloc::Layout, count: usize) -> Option<()> {
        let bytes = layout.size().checked_mul(count)?;
        if bytes == 0 {
            return Some(());
        }
        let offset = self.size.checked_next_multiple_of(layout.align())?;
        self.size = offset.checked_add(bytes)?;
        self.align = self.align.max(layout.align());
        Some(())
    }

    pub(crate) fn layout(self) -> Option<std::alloc::Layout> {
        std::alloc::Layout::from_size_align(self.size, self.align).ok()
    }
}
