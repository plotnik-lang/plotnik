//! Bytecode instruction emission.

use std::collections::BTreeMap;

use crate::bytecode::{
    BYTECODE_WORD_SIZE, Call, CallOwnership, CalleeContract, CodeAddr, Effect, MatchInstr,
    MatchPredicate, Return, SuccessorAddr,
};
use crate::compiler::emit::targets::bytecode::layout_map::LayoutMap;
use crate::compiler::emit::targets::bytecode::tables::{ConstantPool, EmitError};
use crate::compiler::lower::ir::{
    CallIR, CalleeEntryContract, EffectArg, EffectIR, InstructionIR, Label, MatchIR, NfaGraph,
};

pub fn emit_instructions(
    nfa: &NfaGraph,
    layout: &LayoutMap,
    pool: ConstantPool<'_>,
) -> Result<Vec<u8>, EmitError> {
    let mut bytes = vec![0u8; layout.total_words() as usize * BYTECODE_WORD_SIZE];

    for instr in nfa.instructions() {
        let label = instr.label();
        let Some(&code_addr) = layout.code_addrs().get(&label) else {
            continue;
        };

        let offset = u16::from(code_addr) as usize * BYTECODE_WORD_SIZE;
        let resolved = resolve_instruction(instr, nfa, layout.code_addrs(), pool)?;

        let end = offset + resolved.len();
        if end <= bytes.len() {
            bytes[offset..end].copy_from_slice(&resolved);
        }
    }

    Ok(bytes)
}

fn resolve_instruction(
    instr: &InstructionIR,
    nfa: &NfaGraph,
    map: &BTreeMap<Label, CodeAddr>,
    pool: ConstantPool<'_>,
) -> Result<Vec<u8>, EmitError> {
    match instr {
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
            op: pred.op_byte(),
            is_regex: pred.value.is_regex(),
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
    let target = successor_addr(c.target);
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
