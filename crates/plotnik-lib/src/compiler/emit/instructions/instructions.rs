//! Transition instruction emission.

use std::collections::BTreeMap;

use crate::bytecode::{
    Call, Effect, MatchInstr, MatchPredicate, Return, STEP_SIZE, StepAddr, StepId, Trampoline,
};
use crate::compiler::emit::tables::{ConstantPool, EmitError};
use crate::compiler::lower::ir::{
    CallIR, EffectArg, EffectIR, InstructionIR, Label, LayoutMap, MatchIR, MemberRef, TrampolineIR,
};

pub fn emit_instructions(
    instructions: &[InstructionIR],
    layout: &LayoutMap,
    pool: ConstantPool<'_>,
) -> Result<Vec<u8>, EmitError> {
    let mut bytes = vec![0u8; layout.total_steps() as usize * STEP_SIZE];

    for instr in instructions {
        let label = instr.label();
        let Some(&step_id) = layout.step_addrs().get(&label) else {
            continue;
        };

        let offset = u16::from(step_id) as usize * STEP_SIZE;
        let resolved = resolve_instruction(instr, layout.step_addrs(), pool)?;

        let end = offset + resolved.len();
        if end <= bytes.len() {
            bytes[offset..end].copy_from_slice(&resolved);
        }
    }

    Ok(bytes)
}

fn resolve_instruction(
    instr: &InstructionIR,
    map: &BTreeMap<Label, StepAddr>,
    pool: ConstantPool<'_>,
) -> Result<Vec<u8>, EmitError> {
    match instr {
        InstructionIR::Match(m) => resolve_match(m, map, pool),
        InstructionIR::Call(c) => Ok(resolve_call(c, map).to_vec()),
        InstructionIR::Return(_) => Ok(Return::new().to_bytes().to_vec()),
        InstructionIR::Trampoline(t) => Ok(resolve_trampoline(t, map).to_vec()),
    }
}

fn resolve_match(
    m: &MatchIR,
    map: &BTreeMap<Label, StepAddr>,
    pool: ConstantPool<'_>,
) -> Result<Vec<u8>, EmitError> {
    let pre_effects = m
        .pre_effects
        .iter()
        .map(|e| resolve_effect(e, pool))
        .collect();
    let post_effects = m
        .post_effects
        .iter()
        .map(|e| resolve_effect(e, pool))
        .collect();
    let predicate = m.predicate.as_ref().map(|pred| {
        let string_id = pool
            .lookup_str(pred.value.text())
            .expect("predicate string must be interned before transition emission");
        let value_ref = if pred.value.is_regex() {
            pool.lookup_regex(string_id)
                .expect("regex predicate must be interned")
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
        .map(|&l| StepId::try_from(l.resolve(map)).expect("step id must be non-zero"))
        .collect();

    let instr = MatchInstr {
        nav: m.nav,
        node_kind: m.node_kind,
        node_field: m.node_field,
        pre_effects,
        neg_fields: m.neg_fields.clone(),
        post_effects,
        predicate,
        successors,
    };
    instr.encode().map_err(EmitError::from)
}

fn resolve_call(c: &CallIR, map: &BTreeMap<Label, StepAddr>) -> [u8; 8] {
    Call::new(
        c.nav,
        c.node_field,
        StepId::try_from(c.next.resolve(map)).expect("step id must be non-zero"),
        StepId::try_from(c.target.resolve(map)).expect("step id must be non-zero"),
    )
    .to_bytes()
}

fn resolve_trampoline(t: &TrampolineIR, map: &BTreeMap<Label, StepAddr>) -> [u8; 8] {
    Trampoline::new(StepId::try_from(t.next.resolve(map)).expect("step id must be non-zero")).to_bytes()
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
