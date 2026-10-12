use super::CacheAligned;
use crate::bytecode::CodeAddr;
use crate::compiler::lower::ir::{InstructionIR, Label, MatchIR};

#[test]
fn direct_entry_uses_word_zero() {
    let entry = Label(0);
    let instrs = vec![MatchIR::terminal(entry).into()];

    let layout = CacheAligned::layout(&instrs, &[entry]);

    assert_eq!(layout.code_addrs()[&entry], CodeAddr::ZERO);
    assert_eq!(layout.total_words(), 1);
}

#[test]
fn entry_used_as_successor_stays_nonzero() {
    let entry = Label(0);
    let predecessor = Label(1);
    let instrs = vec![
        MatchIR::terminal(entry).into(),
        MatchIR::epsilon(predecessor, entry).into(),
    ];

    let layout = CacheAligned::layout(&instrs, &[entry]);

    assert_eq!(layout.code_addrs()[&entry], CodeAddr::from(1));
}

#[test]
fn total_words_past_u16_does_not_wrap() {
    // 70_000 independent terminal matches pack 8-per-block into 70_000 words,
    // past the u16 ceiling. `total_words` must report the true u32 count so the
    // emitter's overflow guard is reachable instead of silently wrapping.
    let instrs: Vec<InstructionIR> = (0..70_000u32)
        .map(|i| MatchIR::terminal(Label(i)).into())
        .collect();

    let layout = CacheAligned::layout(&instrs, &[]);

    assert!(
        layout.total_words() > u16::MAX as u32,
        "total_words wrapped: {}",
        layout.total_words()
    );
}
