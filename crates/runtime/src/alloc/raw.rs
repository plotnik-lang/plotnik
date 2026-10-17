//! The single allocation path for operation-owned runtime storage.
//!
//! Every block goes through [`allocate`] and [`deallocate`], so allocation
//! failure is reported instead of aborting, and the `failpoints` feature can
//! inject a failure at any one allocation site.

use std::alloc::{Layout, alloc, dealloc};
use std::ptr::NonNull;

/// Allocate a non-empty block, returning `None` when the allocator refuses.
pub(crate) fn allocate(layout: Layout) -> Option<NonNull<u8>> {
    assert!(
        layout.size() > 0,
        "runtime storage never allocates empty blocks"
    );

    #[cfg(feature = "failpoints")]
    if failpoints::should_fail() {
        return None;
    }

    // SAFETY: the layout has a non-zero size.
    let ptr = NonNull::new(unsafe { alloc(layout) })?;

    #[cfg(feature = "failpoints")]
    failpoints::track(layout.size(), true);

    Some(ptr)
}

/// Free a block returned by [`allocate`].
///
/// # Safety
///
/// `ptr` must come from [`allocate`] with this exact `layout` and must not be
/// used afterwards.
pub(crate) unsafe fn deallocate(ptr: NonNull<u8>, layout: Layout) {
    #[cfg(feature = "failpoints")]
    failpoints::track(layout.size(), false);

    // SAFETY: guaranteed by the caller.
    unsafe { dealloc(ptr.as_ptr(), layout) };
}

/// Test hooks that fail one chosen allocation and count live bytes.
///
/// Counters are thread-local: a test drives one operation on one thread and
/// observes exactly that operation's allocations.
#[cfg(feature = "failpoints")]
pub mod failpoints {
    use std::cell::Cell;

    thread_local! {
        static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
        static FAIL_AT: Cell<Option<u64>> = const { Cell::new(None) };
        static LIVE_BYTES: Cell<i64> = const { Cell::new(0) };
    }

    /// Fail the allocation with this zero-based index (counted since the last
    /// [`reset`]); `None` lets every allocation through.
    pub fn fail_at(index: Option<u64>) {
        FAIL_AT.with(|cell| cell.set(index));
    }

    /// Restart the allocation counter.
    pub fn reset() {
        ALLOCATIONS.with(|cell| cell.set(0));
        FAIL_AT.with(|cell| cell.set(None));
    }

    /// Allocation requests seen since the last [`reset`], including the failed one.
    pub fn allocations() -> u64 {
        ALLOCATIONS.with(Cell::get)
    }

    /// Bytes allocated and not yet freed on this thread.
    pub fn live_bytes() -> i64 {
        LIVE_BYTES.with(Cell::get)
    }

    pub(super) fn should_fail() -> bool {
        let index = ALLOCATIONS.with(|cell| {
            let index = cell.get();
            cell.set(index + 1);
            index
        });
        FAIL_AT.with(|cell| cell.get() == Some(index))
    }

    pub(super) fn track(bytes: usize, allocated: bool) {
        let bytes = i64::try_from(bytes).expect("allocation size fits i64");
        LIVE_BYTES.with(|cell| {
            let delta = if allocated { bytes } else { -bytes };
            cell.set(cell.get() + delta);
        });
    }
}
