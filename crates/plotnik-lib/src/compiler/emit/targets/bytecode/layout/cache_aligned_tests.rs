#[test]
fn total_words_past_u16_does_not_wrap() {
    use super::CacheAligned;
    use crate::compiler::lower::ir::{InstructionIR, Label, MatchIR};

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
