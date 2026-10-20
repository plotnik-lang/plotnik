use std::collections::BTreeMap;

use crate::bytecode::{
    BYTECODE_WORD_SIZE, Call, CallOwnership, CalleeContract, CodeAddr, Effect, MatchInstr,
    MatchPredicate, Return, SuccessorAddr, Switch, SwitchEntry,
};
use crate::compiler::emit::targets::bytecode::layout_map::LayoutMap;
use crate::compiler::emit::targets::bytecode::tables::{ConstantPool, EmitError};
use crate::compiler::lower::ir::{
    CallIR, CalleeEntryContract, EffectArg, EffectIR, InstructionIR, Label, MatchIR, NfaGraph,
    SwitchIR,
};

pub fn emit_instructions(
    nfa: &NfaGraph,
    layout: &LayoutMap,
    pool: ConstantPool<'_>,
) -> Result<(Vec<u8>, Vec<u8>), EmitError> {
    let mut bytes = vec![0u8; layout.total_words() as usize * BYTECODE_WORD_SIZE];
    let mut switch_tables = Vec::new();

    for instr in nfa.instructions() {
        let label = instr.label();
        let code_addr = layout
            .code_addrs()
            .get(&label)
            .copied()
            .expect("layout contains every lowered instruction");

        let offset = u16::from(code_addr) as usize * BYTECODE_WORD_SIZE;
        let resolved =
            resolve_instruction(instr, nfa, layout.code_addrs(), pool, &mut switch_tables)?;

        let end = offset + resolved.len();
        bytes[offset..end].copy_from_slice(&resolved);
    }

    Ok((bytes, switch_tables))
}

fn resolve_instruction(
    instr: &InstructionIR,
    nfa: &NfaGraph,
    map: &BTreeMap<Label, CodeAddr>,
    pool: ConstantPool<'_>,
    switch_tables: &mut Vec<u8>,
) -> Result<Vec<u8>, EmitError> {
    match instr {
        InstructionIR::Switch(switch) => resolve_switch(switch, map, switch_tables),
        InstructionIR::Match(m) => resolve_match(m, map, pool),
        InstructionIR::Call(c) => Ok(resolve_call(c, nfa, map)),
        InstructionIR::Return(return_) => {
            let contract = match return_.entry {
                CalleeEntryContract::CallerOwned => CalleeContract::CallerOwned,
                CalleeEntryContract::CalleeOwned { obligation } => CalleeContract::CalleeOwned {
                    nav: obligation.navigation().authored(),
                    node_field: obligation.field(),
                },
            };
            Ok(Return::with_contract(return_.port, contract)
                .to_bytes()
                .to_vec())
        }
    }
}

fn resolve_switch(
    switch: &SwitchIR,
    map: &BTreeMap<Label, CodeAddr>,
    tables: &mut Vec<u8>,
) -> Result<Vec<u8>, EmitError> {
    let mut cases = switch.cases().collect::<Vec<_>>();
    cases.sort_unstable_by_key(|case| case.0);

    let size = tables.len() + cases.len() * SwitchEntry::SIZE;
    if size > u32::MAX as usize {
        return Err(EmitError::SectionTooLarge {
            section: "switch tables",
            size,
        });
    }

    let instruction = Switch {
        table_offset: tables.len() as u32,
        case_count: u16::try_from(cases.len())
            .expect("case count is bounded by the instruction address space"),
    };

    for (kind, target) in cases {
        let target = SuccessorAddr::try_from(target.resolve(map))
            .expect("switch successor address must be non-zero");
        tables.extend_from_slice(&SwitchEntry { kind, target }.to_bytes());
    }

    Ok(instruction.to_bytes().to_vec())
}

fn resolve_match(
    m: &MatchIR,
    map: &BTreeMap<Label, CodeAddr>,
    pool: ConstantPool<'_>,
) -> Result<Vec<u8>, EmitError> {
    let effects = m.effects.iter().map(resolve_effect).collect();
    let predicate = m.predicate.as_ref().map(|pred| {
        let string_id = pool
            .lookup_str(pred.value.text())
            .expect("predicate string must be interned before instruction emission");
        let value_ref = if pred.value.is_regex() {
            u16::from(
                pool.lookup_regex(string_id)
                    .expect("regex predicate must be interned"),
            )
        } else {
            u16::from(string_id)
        };
        MatchPredicate {
            op: pred.op,
            value_ref,
        }
    });
    let successors = m
        .successors
        .iter()
        .map(|&label| {
            SuccessorAddr::try_from(label.resolve(map)).expect("successor address must be non-zero")
        })
        .collect();

    let instr = MatchInstr {
        nav: m.nav,
        node_kind: m.node_kind,
        node_field: m.node_field,
        missing: m.missing,
        effects,
        neg_fields: m.neg_fields.clone(),
        predicate,
        successors,
    };
    instr.encode().map_err(EmitError::Encode)
}

fn resolve_call(c: &CallIR, nfa: &NfaGraph, map: &BTreeMap<Label, CodeAddr>) -> Vec<u8> {
    let successor_addr = |label: Label| {
        SuccessorAddr::try_from(label.resolve(map)).expect("successor address must be non-zero")
    };
    let target = c.target.resolve(map);
    let specialization = nfa
        .specialization_for_entry(c.target)
        .expect("calls target definition specializations");
    assert_eq!(
        c.returns.len(),
        specialization.ports().len(),
        "call continuation count matches callee port signature"
    );
    let consumed_mask = specialization
        .ports()
        .ports()
        .iter()
        .enumerate()
        .fold(0u8, |mask, (index, port)| {
            mask | if port.consumed() { 1 << index } else { 0 }
        });
    let returns = c
        .returns
        .iter()
        .copied()
        .map(successor_addr)
        .collect::<Vec<_>>();
    Call::new(
        if c.entry.caller_owned() {
            CallOwnership::Caller
        } else {
            CallOwnership::Callee
        },
        c.entry.nav(),
        c.entry.field(),
        &returns,
        consumed_mask,
        target,
    )
    .to_bytes()
}

fn resolve_effect(effect: &EffectIR) -> Effect {
    let payload = match effect.payload() {
        EffectArg::Literal(payload) => *payload,
        EffectArg::Member(member) => usize::from(member.raw()),
    };
    Effect::new(effect.kind(), payload)
}
