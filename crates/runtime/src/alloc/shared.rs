use std::alloc::Layout;
use std::ops::Deref;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicUsize, Ordering, fence};

use super::{Account, Meter, raw};
use crate::LimitExceeded;

/// Fallible, accounted shared ownership of immutable operation data.
pub struct Shared<T> {
    ptr: NonNull<Entry<T>>,
}

struct Entry<T> {
    refs: AtomicUsize,
    account: Account,
    value: T,
}

// SAFETY: the count is atomic and the shared value is immutable.
unsafe impl<T: Send + Sync> Send for Shared<T> {}
// SAFETY: access to the value requires its `Sync` contract.
unsafe impl<T: Send + Sync> Sync for Shared<T> {}

impl<T> Shared<T> {
    pub fn new(meter: &mut Meter<'_>, value: T) -> Result<Self, LimitExceeded> {
        let layout = Layout::new::<Entry<T>>();
        let bytes = layout.size() as u64;
        meter.charge(1)?;
        meter.charge_bytes(size_of::<T>() as u64)?;
        let account = meter.account();
        account.reserve(bytes)?;
        let Some(ptr) = raw::allocate(layout) else {
            account.release(bytes);
            return Err(LimitExceeded::Allocation { bytes });
        };
        let ptr = ptr.cast::<Entry<T>>();
        // SAFETY: the admitted block has this exact layout and is uninitialized.
        unsafe {
            ptr.write(Entry {
                refs: AtomicUsize::new(1),
                account: account.clone(),
                value,
            })
        };
        Ok(Self { ptr })
    }
}

impl<T> Deref for Shared<T> {
    type Target = T;

    fn deref(&self) -> &T {
        // SAFETY: this counted reference keeps the entry alive.
        unsafe { &(*self.ptr.as_ptr()).value }
    }
}

impl<T> Clone for Shared<T> {
    fn clone(&self) -> Self {
        // SAFETY: this counted reference keeps the entry alive.
        let previous = unsafe { self.ptr.as_ref() }
            .refs
            .fetch_add(1, Ordering::Relaxed);
        if previous > isize::MAX as usize {
            std::process::abort();
        }
        Self { ptr: self.ptr }
    }
}

impl<T> Drop for Shared<T> {
    fn drop(&mut self) {
        // SAFETY: this reference keeps the entry alive until the decrement.
        if unsafe { self.ptr.as_ref() }
            .refs
            .fetch_sub(1, Ordering::Release)
            != 1
        {
            return;
        }
        fence(Ordering::Acquire);
        // SAFETY: the last owner releases the value and moves out its account
        // before freeing the entry. No memory lock covers either destructor.
        unsafe {
            let ptr = self.ptr.as_ptr();
            let account = (&raw const (*ptr).account).read();
            (&raw mut (*ptr).value).drop_in_place();
            let layout = Layout::new::<Entry<T>>();
            raw::deallocate(self.ptr.cast(), layout);
            account.release(layout.size() as u64);
        }
    }
}
