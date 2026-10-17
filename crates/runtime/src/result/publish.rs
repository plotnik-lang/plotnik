use std::alloc::Layout;

use super::family::{Publish, ResultFamily};
use super::owner::Match;
use super::region::Region;
use super::rows::{Normalizer, Rows};
use super::schema::{Schema, Ty};
use super::scope::{Plan, Scope};
use super::walk::{Step, Walk, WalkStack};
use crate::LimitExceeded;
use crate::alloc::Meter;
use crate::tape::Tape;

/// Reusable scratch that publishes accepted tapes as region-owned result graphs.
///
/// Publication is two linear passes over the normalized rows: the first
/// sizes the region, the second builds the native values into it in
/// postorder, children before parents. Nothing is relocated after a
/// reference to it exists.
pub struct Publisher<'t> {
    normalizer: Normalizer,
    rows: Rows<'t>,
    walk: WalkStack,
}

impl<'t> Publisher<'t> {
    pub fn new() -> Self {
        Self {
            normalizer: Normalizer::new(),
            rows: Rows::new(),
            walk: WalkStack::new(),
        }
    }

    pub fn publish<'s, E: Publish<'t, 's>>(
        &mut self,
        meter: &mut Meter<'_>,
        tape: &Tape<'t>,
        source: &'s str,
    ) -> Result<Match<E>, LimitExceeded> {
        let schema = E::SCHEMA;
        let root = self
            .normalizer
            .run(meter, &schema, tape.events(), &mut self.rows)?;

        let layout = self.plan(meter, schema, E::LAYOUTS, source, root, E::ROOT)?;
        let region = Region::allocate(meter, layout)?;
        let mut walk = Walk::new(
            meter,
            schema,
            &self.rows,
            source,
            &mut self.walk,
            root,
            E::ROOT,
        )?;
        let root = {
            let scope = match region.base() {
                // SAFETY: the region owns `layout.size()` writable bytes at
                // `base`, and `scope` is dropped in this block, before the
                // region can move or release them.
                Some(base) => unsafe { Scope::new(base, layout.size()) },
                None => Scope::empty(),
            };
            let value = E::build(&mut walk, &scope, meter)?;
            meter.charge_bytes(size_of::<E>() as u64)?;
            assert_eq!(
                scope.used(),
                layout.size(),
                "the publisher placed a different amount than planned"
            );
            // SAFETY: `value` references only the region's block and data
            // that outlives `E`; `Match` keeps the block alive and hands the
            // value out only through `view`, which rebinds the storage
            // lifetime to a borrow of the match.
            unsafe { rebind::<E>(value) }
        };
        Ok(Match::new(root, region))
    }

    /// Size the block with the same placement order the build uses: list
    /// elements when the list opens, a referenced value when it closes.
    fn plan(
        &mut self,
        meter: &mut Meter<'_>,
        schema: Schema<'static>,
        layouts: &[Layout],
        source: &str,
        root: u32,
        root_ty: u32,
    ) -> Result<Layout, LimitExceeded> {
        let mut walk = Walk::new(
            meter,
            schema,
            &self.rows,
            source,
            &mut self.walk,
            root,
            root_ty,
        )?;
        let mut plan = Plan::new();
        while let Some(step) = walk.next(meter)? {
            let placement = match step {
                Step::Open { ty, len, .. } => match schema.ty(ty) {
                    Ty::List { element, .. } => Some((element, len)),
                    _ => None,
                },
                Step::Close { ty, .. } => match schema.ty(ty) {
                    Ty::Ref(target) => Some((target, 1)),
                    _ => None,
                },
                _ => None,
            };
            let Some((ty, count)) = placement else {
                continue;
            };
            let layout = layouts[ty as usize];
            plan.place(layout, count)
                .ok_or(LimitExceeded::Allocation { bytes: u64::MAX })?;
        }
        plan.layout()
            .ok_or(LimitExceeded::Allocation { bytes: u64::MAX })
    }
}

impl Default for Publisher<'_> {
    fn default() -> Self {
        Self::new()
    }
}

/// # Safety
///
/// See [`Publisher::publish`]: the caller keeps `value`'s storage alive for
/// as long as it uses the result.
unsafe fn rebind<'a, E: ResultFamily + 'a>(value: E::Borrowed<'a>) -> E {
    assert_eq!(
        size_of::<E::Borrowed<'a>>(),
        size_of::<E>(),
        "a result family rebinds only lifetimes"
    );
    // SAFETY: `Borrowed<'a>` is `E` up to its storage lifetime (the
    // `ResultFamily` contract), so the bytes are a valid `E`.
    unsafe { std::mem::transmute_copy::<E::Borrowed<'a>, E>(&value) }
}

#[cfg(test)]
#[path = "publish_tests.rs"]
mod publish_tests;
