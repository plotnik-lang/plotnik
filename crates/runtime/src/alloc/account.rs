use std::alloc::Layout;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicUsize, Ordering, fence};
use std::sync::{Mutex, MutexGuard, PoisonError};

use super::raw;
use crate::{LimitExceeded, ResolvedRuntimeLimits};

/// The fuel and memory account of one operation.
///
/// An operation (`exec`, one `each` iterator, `find_all`, one JSON write) owns
/// an account. Every buffer, published result block and result table it
/// creates holds a counted reference, so memory stays charged while retained
/// results are alive, even after the operation itself ended, and a result can
/// be dropped on any thread.
///
/// Fuel only grows: rollback, failure and release never refund it. Memory
/// counts requested capacity, including both blocks while a buffer grows.
///
/// Lock order is fuel, then memory. The memory lock is never held across an
/// allocator call or a release of other storage.
pub struct Account {
    shared: NonNull<Shared>,
}

struct Shared {
    refs: AtomicUsize,
    fuel: Mutex<Fuel>,
    memory: Mutex<Memory>,
}

struct Fuel {
    used: u64,
    limit: Option<u64>,
}

struct Memory {
    used: u64,
    peak: u64,
    limit: Option<u64>,
}

// SAFETY: the shared state is only reached through its mutexes and the atomic
// reference count.
unsafe impl Send for Account {}
// SAFETY: as above.
unsafe impl Sync for Account {}

impl Account {
    /// Create an account. Its own allocation is its first memory charge.
    pub fn new(limits: ResolvedRuntimeLimits) -> Result<Self, LimitExceeded> {
        let layout = Layout::new::<Shared>();
        let bytes = layout.size() as u64;
        if let Some(limit) = limits.max_memory
            && bytes > limit
        {
            return Err(LimitExceeded::Memory { used: bytes, limit });
        }

        let ptr = raw::allocate(layout).ok_or(LimitExceeded::Allocation { bytes })?;
        let shared = ptr.cast::<Shared>();
        // SAFETY: `ptr` is a fresh allocation with `Shared`'s layout.
        unsafe {
            shared.write(Shared {
                refs: AtomicUsize::new(1),
                fuel: Mutex::new(Fuel {
                    used: 0,
                    limit: limits.fuel_limit,
                }),
                memory: Mutex::new(Memory {
                    used: bytes,
                    peak: bytes,
                    limit: limits.max_memory,
                }),
            });
        }
        Ok(Self { shared })
    }

    fn shared(&self) -> &Shared {
        // SAFETY: the reference count keeps the shared state alive.
        unsafe { self.shared.as_ref() }
    }

    /// Acquire the exclusive fuel permit for one execution interval.
    ///
    /// Release it (drop the meter) before handing control to caller code
    /// that may use the same account, such as a sink.
    pub fn meter(&self) -> Meter<'_> {
        Meter {
            fuel: Some(lock(&self.shared().fuel)),
            account: self,
        }
    }

    /// Fuel charged so far.
    pub fn fuel_used(&self) -> u64 {
        lock(&self.shared().fuel).used
    }

    /// Memory currently charged, in bytes.
    pub fn memory_used(&self) -> u64 {
        lock(&self.shared().memory).used
    }

    /// The highest memory charge seen, in bytes.
    pub fn memory_peak(&self) -> u64 {
        lock(&self.shared().memory).peak
    }

    pub(crate) fn same(&self, other: &Account) -> bool {
        self.shared == other.shared
    }

    /// Admit `bytes` of new capacity. Never allocates.
    pub(crate) fn reserve(&self, bytes: u64) -> Result<(), LimitExceeded> {
        let mut memory = lock(&self.shared().memory);
        let used = memory
            .used
            .checked_add(bytes)
            .ok_or(LimitExceeded::Allocation { bytes })?;
        if let Some(limit) = memory.limit
            && used > limit
        {
            return Err(LimitExceeded::Memory { used, limit });
        }
        memory.used = used;
        memory.peak = memory.peak.max(used);
        Ok(())
    }

    /// Return capacity whose storage was already freed. Never allocates or frees.
    pub(crate) fn release(&self, bytes: u64) {
        let mut memory = lock(&self.shared().memory);
        memory.used = memory
            .used
            .checked_sub(bytes)
            .expect("released more memory than the account admitted");
    }
}

impl Clone for Account {
    fn clone(&self) -> Self {
        let previous = self.shared().refs.fetch_add(1, Ordering::Relaxed);
        if previous > isize::MAX as usize {
            std::process::abort();
        }
        Self {
            shared: self.shared,
        }
    }
}

impl Drop for Account {
    fn drop(&mut self) {
        if self.shared().refs.fetch_sub(1, Ordering::Release) != 1 {
            return;
        }
        fence(Ordering::Acquire);

        // SAFETY: this was the last reference; the block came from `allocate`
        // with `Shared`'s layout.
        unsafe {
            self.shared.drop_in_place();
            raw::deallocate(self.shared.cast(), Layout::new::<Shared>());
        }
    }
}

/// Bytes of byte-proportional work paid for by one fuel unit.
const BYTES_PER_FUEL: u64 = 64;

/// The exclusive fuel permit of one execution interval.
///
/// Charging is a plain compare and add under the held permit. Memory
/// admission additionally takes the memory lock (fuel, then memory).
pub struct Meter<'a> {
    fuel: Option<MutexGuard<'a, Fuel>>,
    account: &'a Account,
}

impl<'a> Meter<'a> {
    /// Charge `units` of fuel before doing that work.
    #[inline]
    pub fn charge(&mut self, units: u64) -> Result<(), LimitExceeded> {
        let fuel = self.fuel.as_mut().expect("meter holds its fuel permit");
        let used = fuel.used.saturating_add(units);
        if let Some(limit) = fuel.limit
            && used > limit
        {
            return Err(LimitExceeded::OutOfFuel(limit));
        }
        fuel.used = used;
        Ok(())
    }

    /// Charge byte-proportional work: copying, scanning or writing `bytes`.
    #[inline]
    pub fn charge_bytes(&mut self, bytes: u64) -> Result<(), LimitExceeded> {
        self.charge(bytes.div_ceil(BYTES_PER_FUEL))
    }

    pub fn fuel_used(&self) -> u64 {
        self.fuel
            .as_ref()
            .expect("meter holds its fuel permit")
            .used
    }

    #[cfg(feature = "execution")]
    pub(crate) fn remaining(&self) -> u64 {
        let fuel = self.fuel.as_ref().expect("meter holds its fuel permit");
        fuel.limit.unwrap_or(u64::MAX).saturating_sub(fuel.used)
    }

    #[cfg(feature = "execution")]
    pub(crate) fn exhausted(&self) -> LimitExceeded {
        match self
            .fuel
            .as_ref()
            .expect("meter holds its fuel permit")
            .limit
        {
            Some(limit) => LimitExceeded::OutOfFuel(limit),
            None => LimitExceeded::Allocation { bytes: u64::MAX },
        }
    }

    pub fn account(&self) -> &'a Account {
        self.account
    }

    /// Invoke caller code without the fuel permit, then read its current state.
    pub fn callback<R>(&mut self, call: impl FnOnce() -> R) -> R {
        self.fuel = None;
        let result = call();
        self.fuel = Some(lock(&self.account.shared().fuel));
        result
    }
}

/// The account state is plain counters, valid even if a holder panicked.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
