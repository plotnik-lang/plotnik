//! Transition instruction emission.

use std::collections::BTreeMap;

use crate::bytecode::{
    BYTECODE_WORD_SIZE, Call, CodeAddr, Effect, MatchInstr, MatchPredicate, Return, RoutedCall,
    SplitCall, SplitCallReturns, SuccessorAddr,
};
use crate::compiler::emit::targets::bytecode::layout_map::LayoutMap;
use crate::compiler::emit::targets::bytecode::tables::{ConstantPool, EmitError};
use crate::compiler::lower::ir::{
    CallIR, CallProtocol, EffectArg, EffectIR, InstructionIR, Label, MatchIR, MemberRef,
};

pub fn emit_instructions(
    instructions: &[InstructionIR],
    layout: &LayoutMap,
    pool: ConstantPool<'_>,
) -> Result<Vec<u8>, EmitError> {
    let mut bytes = vec![0u8; layout.total_words() as usize * BYTECODE_WORD_SIZE];

    for instr in instructions {
        let label = instr.label();
        let Some(&code_addr) = layout.code_addrs().get(&label) else {
            continue;
        };

        let offset = u16::from(code_addr) as usize * BYTECODE_WORD_SIZE;
        let resolved = resolve_instruction(instr, layout.code_addrs(), pool)?;

        let end = offset + resolved.len();
        if end <= bytes.len() {
            bytes[offset..end].copy_from_slice(&resolved);
        }
    }

    Ok(bytes)
}

fn resolve_instruction(
    instr: &InstructionIR,
    map: &BTreeMap<Label, CodeAddr>,
    pool: ConstantPool<'_>,
) -> Result<Vec<u8>, EmitError> {
    match instr {
        InstructionIR::Match(m) => resolve_match(m, map, pool),
        InstructionIR::Call(c) => Ok(resolve_call(c, map).to_vec()),
        InstructionIR::Return(return_) => {
            let encoded = match return_.mode {
                crate::bytecode::ReturnMode::CallerMatched => Return::matched(),
                crate::bytecode::ReturnMode::RoutedMatched => Return::routed_matched(),
                crate::bytecode::ReturnMode::RoutedZero => Return::routed_zero(),
            };
            Ok(encoded.to_bytes().to_vec())
        }
    }
}

fn resolve_match(
    m: &MatchIR,
    map: &BTreeMap<Label, CodeAddr>,
    pool: ConstantPool<'_>,
) -> Result<Vec<u8>, EmitError> {
    let effects = m.effects.iter().map(|e| resolve_effect(e, pool)).collect();
    let predicate = m.predicate.as_ref().map(|pred| {
        let string_id = pool
            .lookup_str(pred.value.text())
            .expect("predicate string must be interned before transition emission");
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

fn resolve_call(c: &CallIR, map: &BTreeMap<Label, CodeAddr>) -> [u8; 8] {
    let successor_addr = |label: Label| {
        SuccessorAddr::try_from(label.resolve(map)).expect("successor address must be non-zero")
    };
    let target = successor_addr(c.target);
    match c.protocol {
        CallProtocol::Ordinary {
            nav,
            node_field,
            next,
        } => Call::new(nav, node_field, successor_addr(next), target).to_bytes(),
        CallProtocol::Routed { entry_nav, next } => {
            RoutedCall::new(entry_nav, successor_addr(next), target).to_bytes()
        }
        CallProtocol::Split { entry_nav, returns } => SplitCall::new(
            entry_nav,
            SplitCallReturns {
                matched: successor_addr(returns[0]),
                zero: successor_addr(returns[1]),
            },
            target,
        )
        .to_bytes(),
    }
}

fn resolve_effect(effect: &EffectIR, pool: ConstantPool<'_>) -> Effect {
    let payload = match effect.payload() {
        EffectArg::Literal(payload) => *payload,
        EffectArg::Member(member_ref) => resolve_member_ref(*member_ref, pool) as usize,
    };
    Effect::new(effect.kind(), payload)
}

fn resolve_member_ref(member_ref: MemberRef, pool: ConstantPool<'_>) -> u16 {
    pool.member_base(member_ref.parent_type)
        .expect("member base must resolve")
        + member_ref.relative_index
}
