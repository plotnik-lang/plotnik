use crate::bytecode::{EffectKind, MAX_EFFECTS, MAX_MATCH_PAYLOAD_SLOTS, Nav};
use crate::core::NodeFieldId;

use super::lower::pack_instructions;
use crate::compiler::lower::ir::NfaGraph;
use crate::compiler::lower::ir::{EffectIR, InstructionIR, Label, MatchIR};

const MAX_NEG_FIELDS: usize = 7;

fn make_effect(_idx: u16) -> EffectIR {
    EffectIR::literal(EffectKind::Null, 0)
}

#[test]
fn lower_no_overflow_unchanged() {
    let mut result = NfaGraph {
        instructions: vec![
            MatchIR::terminal(Label(0))
                .append_effects((0..3).map(make_effect))
                .next(Label(1))
                .into(),
        ],
        def_entries: Default::default(),
        entrypoint_wrappers: Default::default(),
    };

    pack_instructions(&mut result);

    assert_eq!(result.instructions.len(), 1);
}

#[test]
fn lower_effects_overflow() {
    let mut result = NfaGraph {
        instructions: vec![
            MatchIR::terminal(Label(0))
                .nav(Nav::Down)
                .append_effects((0..18).map(make_effect))
                .next(Label(1))
                .into(),
        ],
        def_entries: Default::default(),
        entrypoint_wrappers: Default::default(),
    };

    pack_instructions(&mut result);

    assert!(result.instructions.len() >= 2);

    for instr in &result.instructions {
        if let InstructionIR::Match(m) = instr {
            assert!(
                m.effects.len() <= MAX_EFFECTS,
                "effects {} > {}",
                m.effects.len(),
                MAX_EFFECTS
            );
        }
    }
}

#[test]
fn lower_neg_fields_overflow() {
    let mut result = NfaGraph {
        instructions: vec![
            MatchIR::terminal(Label(0))
                .nav(Nav::Down)
                .neg_fields((1u16..=10).map(|n| NodeFieldId::try_from(n).unwrap()))
                .next(Label(1))
                .into(),
        ],
        def_entries: Default::default(),
        entrypoint_wrappers: Default::default(),
    };

    pack_instructions(&mut result);

    assert!(result.instructions.len() >= 2);

    for instr in &result.instructions {
        if let InstructionIR::Match(m) = instr {
            assert!(
                m.neg_fields.len() <= MAX_NEG_FIELDS,
                "neg_fields {} > {}",
                m.neg_fields.len(),
                MAX_NEG_FIELDS
            );
        }
    }
}

#[test]
fn lower_successors_overflow() {
    let succs: Vec<_> = (1..=35).map(Label).collect();
    let mut result = NfaGraph {
        instructions: vec![MatchIR::terminal(Label(0)).successors(succs).into()],
        def_entries: Default::default(),
        entrypoint_wrappers: Default::default(),
    };

    pack_instructions(&mut result);

    assert!(result.instructions.len() >= 2);

    for instr in &result.instructions {
        if let InstructionIR::Match(m) = instr {
            assert!(
                m.successors.len() <= MAX_MATCH_PAYLOAD_SLOTS,
                "successors {} > {}",
                m.successors.len(),
                MAX_MATCH_PAYLOAD_SLOTS
            );
        }
    }
}

#[test]
fn lower_successors_overflow_preserves_all_successors() {
    let succs: Vec<_> = (1..=35).map(Label).collect();
    let mut result = NfaGraph {
        instructions: vec![MatchIR::terminal(Label(0)).successors(succs.clone()).into()],
        def_entries: Default::default(),
        entrypoint_wrappers: Default::default(),
    };

    pack_instructions(&mut result);

    // The cascade must append, not replace: every original successor stays reachable
    // from the entry, and in priority order.
    let by_label: std::collections::HashMap<Label, &InstructionIR> =
        result.instructions.iter().map(|i| (i.label(), i)).collect();
    let mut seen = std::collections::HashSet::new();
    let mut stack = vec![Label(0)];
    while let Some(l) = stack.pop() {
        if !seen.insert(l) {
            continue;
        }
        if let Some(instr) = by_label.get(&l) {
            stack.extend(instr.successors());
        }
    }

    for s in succs {
        assert!(seen.contains(&s), "successor {s:?} was dropped by lowering");
    }
}

#[test]
fn lower_successors_with_payload_respect_combined_limit() {
    // Successors share the 28-slot Match64 payload with effects, neg fields, and
    // predicate. Here neither overflows its own sub-limit — 27 successors (≤ 28) and
    // 5 effects (≤ 15) — but together they are 32 slots. Lowering must still keep
    // every instruction within the combined limit, or `MatchIR::resolve` later panics
    // with "instruction too large". Regression for #421.
    let succs: Vec<_> = (1..=27).map(Label).collect();
    let mut result = NfaGraph {
        instructions: vec![
            MatchIR::terminal(Label(0))
                .nav(Nav::Down)
                .append_effects((0..5).map(make_effect))
                .successors(succs.clone())
                .into(),
        ],
        def_entries: Default::default(),
        entrypoint_wrappers: Default::default(),
    };

    pack_instructions(&mut result);

    for instr in &result.instructions {
        if let InstructionIR::Match(m) = instr {
            let predicate_slots = usize::from(m.predicate.is_some()) * 2;
            let slots = m.effects.len() + m.neg_fields.len() + predicate_slots + m.successors.len();
            assert!(
                slots <= MAX_MATCH_PAYLOAD_SLOTS,
                "combined payload {slots} > {MAX_MATCH_PAYLOAD_SLOTS}"
            );
        }
    }

    let by_label: std::collections::HashMap<Label, &InstructionIR> =
        result.instructions.iter().map(|i| (i.label(), i)).collect();
    let mut seen = std::collections::HashSet::new();
    let mut stack = vec![Label(0)];
    while let Some(l) = stack.pop() {
        if !seen.insert(l) {
            continue;
        }
        if let Some(instr) = by_label.get(&l) {
            stack.extend(instr.successors());
        }
    }
    for s in succs {
        assert!(seen.contains(&s), "successor {s:?} was dropped by lowering");
    }
}

#[test]
fn lower_combined_overflow() {
    let mut result = NfaGraph {
        instructions: vec![
            MatchIR::terminal(Label(0))
                .nav(Nav::Down)
                .append_effects((0..20).map(make_effect))
                .neg_fields((1u16..=10).map(|n| NodeFieldId::try_from(n).unwrap()))
                .next(Label(1))
                .into(),
        ],
        def_entries: Default::default(),
        entrypoint_wrappers: Default::default(),
    };

    pack_instructions(&mut result);

    for instr in &result.instructions {
        if let InstructionIR::Match(m) = instr {
            assert!(m.effects.len() <= MAX_EFFECTS);
            assert!(m.neg_fields.len() <= MAX_NEG_FIELDS);
            assert!(m.successors.len() <= MAX_MATCH_PAYLOAD_SLOTS);
        }
    }
}
