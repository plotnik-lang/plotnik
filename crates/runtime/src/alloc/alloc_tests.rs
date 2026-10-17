use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use super::{Account, Buffer, Shared, Text};
use crate::{LimitExceeded, ResolvedRuntimeLimits};

fn account() -> Account {
    Account::new(ResolvedRuntimeLimits {
        fuel_limit: None,
        max_memory: None,
    })
    .unwrap()
}

#[test]
fn shared_values_release_once_outside_account_locks_under_concurrent_drop() {
    struct Probe {
        drops: Arc<AtomicUsize>,
        account: Account,
    }
    impl Drop for Probe {
        fn drop(&mut self) {
            assert!(self.account.memory_used() > 0);
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }
    let account = account();
    let baseline = account.memory_used();
    let drops = Arc::new(AtomicUsize::new(0));
    let shared = Shared::new(
        &mut account.meter(),
        Probe {
            drops: drops.clone(),
            account: account.clone(),
        },
    )
    .unwrap();

    std::thread::scope(|scope| {
        for _ in 0..8 {
            let shared = shared.clone();
            scope.spawn(move || {
                for _ in 0..1000 {
                    drop(shared.clone());
                }
                drop(shared);
            });
        }
        drop(shared);
    });

    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(account.memory_used(), baseline);
}

#[test]
fn callback_spending_is_authoritative_and_release_does_not_refund_fuel() {
    let account = Account::new(ResolvedRuntimeLimits {
        fuel_limit: Some(9),
        max_memory: None,
    })
    .unwrap();
    let baseline = account.memory_used();
    let mut meter = account.meter();
    meter.charge(3).unwrap();

    meter.callback(|| {
        let mut output = Text::new(&account);
        output.push(&mut account.meter(), "abc").unwrap();
        assert!(account.memory_used() > baseline);
    });

    assert_eq!(account.memory_used(), baseline);
    assert_eq!(meter.fuel_used(), 5);
    meter.charge(4).unwrap();
    assert_eq!(meter.charge(1), Err(LimitExceeded::OutOfFuel(9)));
    meter.callback(|| assert_eq!(account.fuel_used(), 9));
}

#[test]
fn buffer_growth_charges_both_blocks_and_release_returns_capacity() {
    let account = account();
    let baseline = account.memory_used();
    let mut buffer = Buffer::new();
    buffer.push(&mut account.meter(), 41u64).unwrap();
    let old = buffer.capacity_bytes();
    let count = old as usize / size_of::<u64>();
    for index in 1..=count {
        buffer.push(&mut account.meter(), index as u64).unwrap();
    }

    assert_eq!(account.memory_used(), baseline + buffer.capacity_bytes());
    assert_eq!(
        account.memory_peak(),
        baseline + old + buffer.capacity_bytes()
    );
    assert_eq!(buffer.as_slice()[0], 41);
    assert_eq!(buffer.as_slice()[count], count as u64);
    buffer.clear();
    assert_eq!(account.memory_used(), baseline + buffer.capacity_bytes());
    drop(buffer);
    assert_eq!(account.memory_used(), baseline);
}

#[cfg(feature = "failpoints")]
#[test]
fn shared_allocation_failure_preserves_error_and_drops_input_storage() {
    use super::failpoints;
    let account = account();
    let baseline = account.memory_used();
    let mut input = Buffer::new();
    input.push(&mut account.meter(), 73u64).unwrap();
    failpoints::reset();
    failpoints::fail_at(Some(0));

    let result = Shared::new(&mut account.meter(), input);

    assert!(matches!(result, Err(LimitExceeded::Allocation { .. })));
    assert_eq!(account.memory_used(), baseline);
    assert_eq!(failpoints::allocations(), 1);
    failpoints::reset();
}
