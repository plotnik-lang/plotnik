use std::alloc::Layout;

use super::{Match, Matches};
use crate::alloc::{Account, Meter};
use crate::result::lease::Lease;
use crate::{ResolvedRuntimeLimits, Scope};

#[derive(Clone, Copy)]
struct Row<'r> {
    first: &'r u64,
    items: &'r [u64],
    last: &'r u64,
}

crate::result_family!(Row<'r>);

fn make<'r>(meter: &mut Meter<'_>, seed: u64) -> Match<Row<'r>> {
    let layout = Layout::array::<u64>(5).unwrap();
    let lease = Lease::allocate(meter, layout).unwrap();
    // SAFETY: the returned match owns the lease and bounds all exposed borrows.
    let scope = unsafe { Scope::new(lease.base().unwrap(), layout.size()) };
    let first = scope.push(meter, seed).unwrap();
    let mut items = scope.slice(3);
    items.write(meter, seed + 11).unwrap();
    let last = scope.push(meter, seed + 29).unwrap();
    items.write(meter, seed + 17).unwrap();
    items.write(meter, seed + 23).unwrap();
    let items = items.finish();

    assert_eq!(*first, seed);
    assert_eq!(items, [seed + 11, seed + 17, seed + 23]);
    assert_eq!(*last, seed + 29);

    Match::new(Row { first, items, last }, lease)
}

#[test]
fn borrowed_rows_survive_moves_growth_and_cross_thread_release() {
    let account = Account::new(ResolvedRuntimeLimits {
        fuel_limit: None,
        max_memory: None,
    })
    .unwrap();
    let baseline = account.memory_used();
    let mut matches = Matches::new();
    {
        let mut meter = account.meter();
        for seed in 0..19 {
            let result = make(&mut meter, seed);
            matches.push(&mut meter, result).unwrap();
        }
    }

    for (seed, row) in (&matches).into_iter().enumerate() {
        let seed = seed as u64;
        assert_eq!(*row.first, seed);
        assert_eq!(row.items, [seed + 11, seed + 17, seed + 23]);
        assert_eq!(*row.last, seed + 29);
    }
    std::thread::scope(|threads| {
        threads.spawn(move || drop(matches)).join().unwrap();
    });

    assert_eq!(account.memory_used(), baseline);
}

#[test]
fn single_owner_outlives_its_meter_and_moves_between_threads() {
    let account = Account::new(ResolvedRuntimeLimits {
        fuel_limit: None,
        max_memory: None,
    })
    .unwrap();
    let baseline = account.memory_used();
    let result = make(&mut account.meter(), 37);

    std::thread::scope(|threads| {
        threads
            .spawn(move || {
                let row = result.view();
                assert_eq!(*row.first, 37);
                assert_eq!(row.items, [48, 54, 60]);
                assert_eq!(*row.last, 66);
                drop(result);
            })
            .join()
            .unwrap();
    });

    assert_eq!(account.memory_used(), baseline);
}
