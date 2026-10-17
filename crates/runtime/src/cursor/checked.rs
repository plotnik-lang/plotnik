use std::alloc::Layout;
use std::ffi::c_void;
use std::ptr::{self, NonNull};

use tree_sitter::{Node, ffi};

use crate::LimitExceeded;
use crate::alloc::{Account, Meter, raw};

type Grow = unsafe extern "C" fn(
    *mut c_void,
    *mut *mut c_void,
    *mut u32,
    u32,
    u32,
    usize,
    usize,
    *mut u64,
) -> bool;

unsafe extern "C" {
    fn plotnik_cursor_node(cursor: *const ffi::TSTreeCursor) -> ffi::TSNode;
    fn plotnik_cursor_index(cursor: *const ffi::TSTreeCursor) -> u32;
    fn plotnik_cursor_run(
        cursor: *mut ffi::TSTreeCursor,
        operation: u32,
        node: ffi::TSNode,
        operand: u32,
        source: *const ffi::TSTreeCursor,
        fuel: *mut u64,
        grow: Grow,
        context: *mut c_void,
    ) -> i32;
}

/// Binding-owned storage. C only initializes entries and never allocates or
/// frees. The private layout comes from the exact tree-sitter dependency.
struct Storage {
    block: Option<(NonNull<u8>, Layout)>,
    account: Option<Account>,
}

struct Context<'a, 'm> {
    storage: &'a mut Storage,
    meter: &'a mut Meter<'m>,
    start: u64,
    error: Option<LimitExceeded>,
}

impl Context<'_, '_> {
    fn grow(
        &mut self,
        count: u32,
        required: u32,
        size: usize,
        align: usize,
        fuel: &mut u64,
    ) -> Result<(NonNull<u8>, u32), LimitExceeded> {
        self.meter.charge(self.start - *fuel)?;
        self.start = *fuel;
        let overflow = LimitExceeded::Allocation { bytes: u64::MAX };
        let old_cap = self
            .storage
            .block
            .map_or(0, |(_, layout)| layout.size() / size);
        let cap = old_cap
            .checked_mul(2)
            .ok_or(overflow)?
            .max(required as usize)
            .max(8);
        let cap = u32::try_from(cap).map_err(|_| overflow)?;
        let bytes = (cap as usize).checked_mul(size).ok_or(overflow)?;
        let layout = Layout::from_size_align(bytes, align).map_err(|_| overflow)?;
        self.meter.charge(1)?;
        self.meter.charge_bytes(u64::from(count) * size as u64)?;
        *fuel = self.meter.remaining();
        self.start = *fuel;
        let account = self.meter.account();
        account.reserve(bytes as u64)?;
        let Some(ptr) = raw::allocate(layout) else {
            account.release(bytes as u64);
            return Err(LimitExceeded::Allocation {
                bytes: bytes as u64,
            });
        };
        if let Some((old, old_layout)) = self.storage.block {
            // SAFETY: C's initialized prefix fits the old and the new block.
            unsafe {
                ptr.as_ptr()
                    .copy_from_nonoverlapping(old.as_ptr(), count as usize * size);
                raw::deallocate(old, old_layout);
            }
            account.release(old_layout.size() as u64);
        }
        self.storage.block = Some((ptr, layout));
        if self.storage.account.is_none() {
            self.storage.account = Some(account.clone());
        }
        Ok((ptr, cap))
    }
}

unsafe extern "C" fn grow(
    context: *mut c_void,
    data: *mut *mut c_void,
    capacity: *mut u32,
    count: u32,
    required: u32,
    size: usize,
    align: usize,
    fuel: *mut u64,
) -> bool {
    // SAFETY: only the synchronous companion call invokes this callback, with
    // a live exclusive context and pointers into its destination cursor.
    let context = unsafe { &mut *context.cast::<Context<'_, '_>>() };
    match context.grow(count, required, size, align, unsafe { &mut *fuel }) {
        Ok((ptr, cap)) => {
            unsafe {
                data.write(ptr.as_ptr().cast());
                capacity.write(cap);
            }
            true
        }
        Err(error) => {
            context.error = Some(error);
            false
        }
    }
}

impl Drop for Storage {
    fn drop(&mut self) {
        if let Some((ptr, layout)) = self.block {
            // SAFETY: this storage owns the allocation, not its C view.
            unsafe { raw::deallocate(ptr, layout) };
            self.account
                .as_ref()
                .expect("allocated storage has an account")
                .release(layout.size() as u64);
        }
    }
}

pub(crate) struct CheckedCursor<'t> {
    raw: ffi::TSTreeCursor,
    storage: Storage,
    root: Node<'t>,
    node: Node<'t>,
    initialized: bool,
}

// SAFETY: storage is exclusive, tree nodes are immutable, and accounting is synchronized.
unsafe impl Send for CheckedCursor<'_> {}
unsafe impl Sync for CheckedCursor<'_> {}

impl<'t> CheckedCursor<'t> {
    pub fn new(node: Node<'t>) -> Self {
        Self {
            // SAFETY: a zeroed C cursor has no stack entries and owns nothing.
            raw: unsafe { std::mem::zeroed() },
            storage: Storage {
                block: None,
                account: None,
            },
            root: node,
            node,
            initialized: false,
        }
    }

    pub fn reset(&mut self, node: Node<'t>) {
        self.root = node;
        self.node = node;
        self.initialized = false;
    }

    pub fn node(&self) -> Node<'t> {
        self.node
    }

    pub fn capacity_bytes(&self) -> u64 {
        self.storage
            .block
            .map_or(0, |(_, layout)| layout.size() as u64)
    }

    pub fn index(&self) -> u32 {
        if !self.initialized {
            return 0;
        }
        // SAFETY: initialization installed a root and successful movement leaves a valid path.
        unsafe { plotnik_cursor_index(&self.raw) }
    }

    fn run(
        &mut self,
        meter: &mut Meter<'_>,
        operation: u32,
        operand: u32,
        source: *const ffi::TSTreeCursor,
    ) -> Result<bool, LimitExceeded> {
        let mut fuel = meter.remaining();
        let mut context = Context {
            storage: &mut self.storage,
            meter,
            start: fuel,
            error: None,
        };
        // SAFETY: C uses the exact dependency's cursor layout and accesses only
        // binding-owned storage. No callback unwinds or outlives this call.
        let result = unsafe {
            plotnik_cursor_run(
                &mut self.raw,
                operation,
                self.root.into_raw(),
                operand,
                source,
                &mut fuel,
                grow,
                (&raw mut context).cast(),
            )
        };
        context.meter.charge(context.start - fuel)?;
        if let Some(error) = context.error {
            self.initialized = false;
            return Err(error);
        }
        if result < 0 {
            self.initialized = false;
            return Err(context.meter.exhausted());
        }
        self.initialized = true;
        // SAFETY: every successful operation preserves a nonempty path tied to 't.
        self.node = unsafe { Node::from_raw(plotnik_cursor_node(&self.raw)) };
        Ok(result != 0)
    }

    fn ready(&mut self, meter: &mut Meter<'_>) -> Result<(), LimitExceeded> {
        if !self.initialized {
            self.run(meter, 0, 0, ptr::null())?;
        }
        Ok(())
    }

    pub fn first_child(&mut self, meter: &mut Meter<'_>) -> Result<bool, LimitExceeded> {
        self.ready(meter)?;
        self.run(meter, 1, 0, ptr::null())
    }
    pub fn next_sibling(&mut self, meter: &mut Meter<'_>) -> Result<bool, LimitExceeded> {
        self.ready(meter)?;
        self.run(meter, 2, 0, ptr::null())
    }
    pub fn parent(&mut self, meter: &mut Meter<'_>) -> Result<bool, LimitExceeded> {
        self.ready(meter)?;
        self.run(meter, 3, 0, ptr::null())
    }
    pub fn seek(&mut self, meter: &mut Meter<'_>, index: u32) -> Result<(), LimitExceeded> {
        self.ready(meter)?;
        self.run(meter, 4, index, ptr::null())?;
        Ok(())
    }
    pub fn copy_from(&mut self, meter: &mut Meter<'_>, source: &Self) -> Result<(), LimitExceeded> {
        if !source.initialized {
            self.reset(source.node);
            return Ok(());
        }
        self.root = source.root;
        self.run(meter, 5, 0, &source.raw)?;
        Ok(())
    }
    pub fn has_field(&mut self, meter: &mut Meter<'_>, field: u16) -> Result<bool, LimitExceeded> {
        self.ready(meter)?;
        self.run(meter, 6, u32::from(field), ptr::null())
    }
    pub fn path_extra(&mut self, meter: &mut Meter<'_>) -> Result<bool, LimitExceeded> {
        self.ready(meter)?;
        self.run(meter, 7, 0, ptr::null())
    }
}
