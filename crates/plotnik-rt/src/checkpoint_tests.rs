//! Tests for `CheckpointStack`'s `max_frame_idx` invariant.
//!
//! `max_frame_idx` must always equal the maximum `frame_index` over the live
//! checkpoints. The prefix-max (`max_frame_idx_below`) maintenance must preserve this
//! exactly — including the cases that previously forced an O(n) rescan:
//! duplicate max-holders and the all-`None` stack.

use crate::checkpoint::{Checkpoint, CheckpointStack, CheckpointState};

fn cp(frame_index: Option<u32>) -> Checkpoint {
    Checkpoint::branch(
        CheckpointState {
            descendant_index: 0,
            effect_watermark: 0,
            frame_index,
            recursion_depth: 0,
            suppress_depth: 0,
        },
        0,
    )
}

fn brute_force_max(frames: &[Option<u32>]) -> Option<u32> {
    frames.iter().copied().flatten().max()
}

/// Push every frame index, then pop them all, asserting after every operation
/// that `max_frame_idx()` matches a brute-force recomputation over the live set.
fn check_sequence(frames: &[Option<u32>]) {
    let mut stack = CheckpointStack::new();
    let mut live: Vec<Option<u32>> = Vec::new();

    for &f in frames {
        stack.push(cp(f));
        live.push(f);
        assert_eq!(
            stack.max_frame_idx(),
            brute_force_max(&live),
            "max_frame_idx diverged after push of {f:?} (live={live:?})"
        );
    }

    while !live.is_empty() {
        let popped = stack.pop().expect("non-empty").state.frame_index;
        let expected = live.pop().unwrap();
        assert_eq!(popped, expected, "pop returned the wrong checkpoint");
        assert_eq!(
            stack.max_frame_idx(),
            brute_force_max(&live),
            "max_frame_idx diverged after pop (live={live:?})"
        );
    }

    assert_eq!(stack.max_frame_idx(), None, "empty stack must have no max");
}

#[test]
fn all_none_frames() {
    // The pre-fix code rescanned the whole stack on every pop here (None == None).
    check_sequence(&[None, None, None, None, None]);
}

#[test]
fn duplicate_max_holders() {
    // Several checkpoints share the maximum frame index — the case that defeated
    // the old "O(1) amortized" claim.
    check_sequence(&[Some(2), Some(5), Some(5), Some(5), Some(3)]);
}

#[test]
fn mixed_none_and_some() {
    check_sequence(&[None, Some(1), None, Some(4), Some(4), None, Some(2)]);
}

#[test]
fn monotonic_increasing_then_decreasing() {
    check_sequence(&[Some(0), Some(1), Some(2), Some(3), Some(4)]);
    check_sequence(&[Some(4), Some(3), Some(2), Some(1), Some(0)]);
}

#[test]
fn interleaved_push_pop() {
    let mut stack = CheckpointStack::new();
    let mut live: Vec<Option<u32>> = Vec::new();
    let ops: &[(bool, Option<u32>)] = &[
        (true, Some(3)),
        (true, Some(7)),
        (false, None),
        (true, Some(7)),
        (true, None),
        (false, None),
        (false, None),
        (true, Some(1)),
        (false, None),
    ];

    for &(push, f) in ops {
        if push {
            stack.push(cp(f));
            live.push(f);
        } else if !live.is_empty() {
            stack.pop();
            live.pop();
        }
        assert_eq!(stack.max_frame_idx(), brute_force_max(&live));
    }
}

/// Regression: the suppression counter must outrange `u16`. Deep `@_` recursion
/// increments `VM::suppress_depth` once per open scope and overflowed the former
/// `u16` at 65_536 — a panic in debug, a silent wrap in release — on valid input.
/// A checkpoint snapshots that counter, so this field is type-locked to the VM's;
/// pinning a value past `u16::MAX` here fails to compile if either narrows again.
#[test]
fn suppress_depth_outranges_u16() {
    let state = CheckpointState {
        descendant_index: 0,
        effect_watermark: 0,
        frame_index: None,
        recursion_depth: 0,
        suppress_depth: u16::MAX as u64 + 1,
    };
    assert_eq!(state.suppress_depth, 65_536);
}
