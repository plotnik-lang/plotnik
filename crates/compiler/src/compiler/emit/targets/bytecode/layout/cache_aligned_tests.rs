use super::CacheAligned;
use crate::bytecode::CodeAddr;
use crate::compiler::emit::targets::bytecode::tables::EmitError;
use crate::compiler::lower::ir::{InstructionIR, Label, MatchIR};

#[test]
fn direct_entry_uses_word_zero() {
    let entry = Label(0);
    let instrs = vec![MatchIR::terminal(entry).into()];

    let layout = CacheAligned::layout(&instrs, &[entry]).expect("layout fits");

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

    let layout = CacheAligned::layout(&instrs, &[entry]).expect("layout fits");

    assert_eq!(layout.code_addrs()[&entry], CodeAddr::from(1));
}

#[test]
fn layout_word_limit() {
    for (words, fits) in [(65_535u32, true), (65_536, false), (70_000, false)] {
        let instrs: Vec<InstructionIR> = (0..words)
            .map(|i| MatchIR::terminal(Label(i)).into())
            .collect();

        let result = CacheAligned::layout(&instrs, &[]);

        match (fits, result) {
            (true, Ok(layout)) => {
                assert_eq!(layout.total_words(), 65_535);
                assert_eq!(layout.code_addrs()[&Label(65_534)], CodeAddr::from(65_534));
            }
            (false, Err(EmitError::TooManyInstructionWords(actual))) => {
                assert_eq!(actual, words as usize);
            }
            (_, result) => panic!("unexpected layout result for {words} words: {result:?}"),
        }
    }
}
