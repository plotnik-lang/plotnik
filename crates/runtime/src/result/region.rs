use std::alloc::Layout;
use std::ptr::NonNull;

use crate::LimitExceeded;
use crate::alloc::{Account, Meter, raw};

/// The region holding a native result graph's descendants and list elements.
///
/// The root stays inline in `Match`. The region holds only `Copy` rows, so
/// releasing it never visits them. A graph without descendants or list elements
/// needs no block but still keeps the operation account for later rendering.
pub(crate) struct Region {
    block: Option<(NonNull<u8>, Layout)>,
    account: Account,
}

// SAFETY: the region exclusively owns its block; the account is thread-safe.
unsafe impl Send for Region {}
// SAFETY: shared access never touches the block through the region.
unsafe impl Sync for Region {}

impl Region {
    /// Admit and allocate a block of `layout`; a zero-size layout allocates nothing.
    pub(crate) fn allocate(meter: &mut Meter<'_>, layout: Layout) -> Result<Self, LimitExceeded> {
        let account = meter.account().clone();
        if layout.size() == 0 {
            return Ok(Self {
                block: None,
                account,
            });
        }

        let bytes = layout.size() as u64;
        meter.charge(1)?;
        account.reserve(bytes)?;
        let Some(ptr) = raw::allocate(layout) else {
            account.release(bytes);
            return Err(LimitExceeded::Allocation { bytes });
        };
        Ok(Self {
            block: Some((ptr, layout)),
            account,
        })
    }

    pub(crate) fn base(&self) -> Option<NonNull<u8>> {
        self.block.map(|(ptr, _)| ptr)
    }

    pub(crate) fn account(&self) -> &Account {
        &self.account
    }
}

impl Drop for Region {
    fn drop(&mut self) {
        let Some((ptr, layout)) = self.block.take() else {
            return;
        };
        // SAFETY: the block came from `allocate` with this layout, and every
        // reference into it is bounded by the owner of this region.
        unsafe { raw::deallocate(ptr, layout) };
        self.account.release(layout.size() as u64);
    }
}
