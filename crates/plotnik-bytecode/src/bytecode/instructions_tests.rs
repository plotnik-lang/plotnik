//! Tests for bytecode instructions.

use std::num::NonZeroU16;

use super::instructions::{Call, Opcode, Return, StepId, align_to_section, select_match_opcode};
use super::nav::Nav;

#[test]
fn opcode_sizes() {
    assert_eq!(Opcode::Match8.size(), 8);
    assert_eq!(Opcode::Match16.size(), 16);
    assert_eq!(Opcode::Match24.size(), 24);
    assert_eq!(Opcode::Match32.size(), 32);
    assert_eq!(Opcode::Match48.size(), 48);
    assert_eq!(Opcode::Match64.size(), 64);
    assert_eq!(Opcode::Call.size(), 8);
    assert_eq!(Opcode::Return.size(), 8);
}

#[test]
fn opcode_step_counts() {
    assert_eq!(Opcode::Match8.step_count(), 1);
    assert_eq!(Opcode::Match16.step_count(), 2);
    assert_eq!(Opcode::Match32.step_count(), 4);
    assert_eq!(Opcode::Match64.step_count(), 8);
}

#[test]
fn opcode_payload_slots() {
    assert_eq!(Opcode::Match8.payload_slots(), 0);
    assert_eq!(Opcode::Match16.payload_slots(), 4);
    assert_eq!(Opcode::Match24.payload_slots(), 8);
    assert_eq!(Opcode::Match32.payload_slots(), 12);
    assert_eq!(Opcode::Match48.payload_slots(), 20);
    assert_eq!(Opcode::Match64.payload_slots(), 28);
}

#[test]
fn select_match_opcode_picks_smallest() {
    assert_eq!(select_match_opcode(0), Some(Opcode::Match8));
    assert_eq!(select_match_opcode(1), Some(Opcode::Match16));
    assert_eq!(select_match_opcode(4), Some(Opcode::Match16));
    assert_eq!(select_match_opcode(5), Some(Opcode::Match24));
    assert_eq!(select_match_opcode(12), Some(Opcode::Match32));
    assert_eq!(select_match_opcode(20), Some(Opcode::Match48));
    assert_eq!(select_match_opcode(28), Some(Opcode::Match64));
    assert_eq!(select_match_opcode(29), None);
}

#[test]
fn align_to_section_works() {
    assert_eq!(align_to_section(0), 0);
    assert_eq!(align_to_section(1), 64);
    assert_eq!(align_to_section(64), 64);
    assert_eq!(align_to_section(65), 128);
    assert_eq!(align_to_section(100), 128);
}

#[test]
fn call_roundtrip() {
    let c = Call::new(
        Nav::Down,
        NonZeroU16::new(42),
        StepId::new(100),
        StepId::new(500),
    );

    let bytes = c.to_bytes();
    let decoded = Call::from_bytes(bytes);
    assert_eq!(decoded, c);
}

#[test]
fn return_roundtrip() {
    let r = Return::new();

    let bytes = r.to_bytes();
    let decoded = Return::from_bytes(bytes);
    assert_eq!(decoded, r);
}
