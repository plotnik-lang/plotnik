//! Transition instruction emission.

use std::collections::BTreeMap;

use plotnik_bytecode::{
    Call, Effect, MatchInstr, MatchPredicate, Return, STEP_SIZE, StepAddr, StepId, Trampoline,
};
use plotnik_compiler_core::ir::{
    CallIR, EffectArg, EffectIR, InstructionIR, Label, LayoutMap, MatchIR, MemberRef, TrampolineIR,
};
use plotnik_compiler_core::{EmitError, RegexTableBuilder, StringTableBuilder, TypeTableBuilder};

pub fn emit_instructions(
    instructions: &[InstructionIR],
    layout: &LayoutMap,
    types: &TypeTableBuilder,
    strings: &StringTableBuilder,
    regexes: &RegexTableBuilder,
) -> Result<Vec<u8>, EmitError> {
    let mut bytes = vec![0u8; layout.total_steps() as usize * STEP_SIZE];

    let ctx = ResolveCtx {
        types,
        strings,
        regexes,
    };

    for instr in instructions {
        let label = instr.label();
        let Some(&step_id) = layout.step_addrs().get(&label) else {
            continue;
        };

        let offset = step_id as usize * STEP_SIZE;
        let resolved = resolve_instruction(instr, layout.step_addrs(), &ctx)?;

        let end = offset + resolved.len();
        if end <= bytes.len() {
            bytes[offset..end].copy_from_slice(&resolved);
        }
    }

    Ok(bytes)
}

struct ResolveCtx<'a> {
    types: &'a TypeTableBuilder,
    strings: &'a StringTableBuilder,
    regexes: &'a RegexTableBuilder,
}

fn resolve_instruction(
    instr: &InstructionIR,
    map: &BTreeMap<Label, StepAddr>,
    ctx: &ResolveCtx<'_>,
) -> Result<Vec<u8>, EmitError> {
    match instr {
        InstructionIR::Match(m) => resolve_match(m, map, ctx),
        InstructionIR::Call(c) => Ok(resolve_call(c, map).to_vec()),
        InstructionIR::Return(_) => Ok(Return::new().to_bytes().to_vec()),
        InstructionIR::Trampoline(t) => Ok(resolve_trampoline(t, map).to_vec()),
    }
}

fn resolve_match(
    m: &MatchIR,
    map: &BTreeMap<Label, StepAddr>,
    ctx: &ResolveCtx<'_>,
) -> Result<Vec<u8>, EmitError> {
    let pre_effects = m
        .pre_effects
        .iter()
        .map(|e| resolve_effect(e, ctx))
        .collect();
    let post_effects = m
        .post_effects
        .iter()
        .map(|e| resolve_effect(e, ctx))
        .collect();
    let predicate = m.predicate.as_ref().map(|pred| {
        let string_id = ctx
            .strings
            .lookup_str(pred.value.text())
            .expect("predicate string must be interned before transition emission");
        let value_ref = if pred.value.is_regex() {
            ctx.regexes
                .lookup(string_id)
                .expect("regex predicate must be interned")
        } else {
            string_id.as_u16()
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
        .map(|&l| StepId::new(l.resolve(map)))
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
        StepId::new(c.next.resolve(map)),
        StepId::new(c.target.resolve(map)),
    )
    .to_bytes()
}

fn resolve_trampoline(t: &TrampolineIR, map: &BTreeMap<Label, StepAddr>) -> [u8; 8] {
    Trampoline::new(StepId::new(t.next.resolve(map))).to_bytes()
}

fn resolve_effect(effect: &EffectIR, ctx: &ResolveCtx<'_>) -> Effect {
    let payload = match effect.payload() {
        EffectArg::Literal(payload) => *payload,
        EffectArg::Member(member_ref) => resolve_member_ref(*member_ref, ctx) as usize,
    };
    Effect::new(effect.kind(), payload)
}

fn resolve_member_ref(member_ref: MemberRef, ctx: &ResolveCtx<'_>) -> u16 {
    ctx.types
        .get_member_base(member_ref.parent_type)
        .expect("member base must resolve")
        + member_ref.relative_index
}
