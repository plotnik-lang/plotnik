//! Lowering pass: transforms unconstrained IR into bytecode-compatible form.
//!
//! This pass handles bytecode encoding constraints by splitting oversized
//! instructions into cascading epsilon chains:
//! - pre_effects > 7 → epsilon chain before match
//! - post_effects > 7 → epsilon chain after match
//! - neg_fields > 7 → epsilon chain for overflow checks
//! - successors > 28 → cascading epsilon branches

use crate::bytecode::{MAX_MATCH_PAYLOAD_SLOTS, MAX_PRE_EFFECTS};

use crate::compiler::lower::ir::{CompileResult, EffectIR, InstructionIR, Label, MatchIR};

const MAX_POST_EFFECTS: usize = 7;
const MAX_NEG_FIELDS: usize = 7;

enum PostChain {
    NegFields(Vec<u16>),
    PostEffects(Vec<EffectIR>),
    Successors(Vec<Label>),
}

pub fn lower(result: &mut CompileResult) {
    let mut next_label = result
        .instructions
        .iter()
        .map(|i| i.label().0)
        .max()
        .unwrap_or(0)
        + 1;

    let mut new_instructions = Vec::with_capacity(result.instructions.len());

    for instr in result.instructions.drain(..) {
        let InstructionIR::Match(m) = instr else {
            new_instructions.push(instr);
            continue;
        };
        lower_match(m, &mut new_instructions, &mut next_label);
    }

    result.instructions = new_instructions;
}

fn fresh_label(next: &mut u32) -> Label {
    let l = Label(*next);
    *next += 1;
    l
}

fn lower_match(mut m: MatchIR, out: &mut Vec<InstructionIR>, next_label: &mut u32) {
    if m.pre_effects.len() > MAX_PRE_EFFECTS {
        let all_pre = std::mem::take(&mut m.pre_effects);
        let entry = m.label;
        m.label = fresh_label(next_label);
        emit_effects_chain(entry, m.label, all_pre, out, next_label);
    }

    let post_chains = drain_post_chains(&mut m);
    if post_chains.is_empty() {
        out.push(m.into());
        return;
    }

    let mut current_succs = std::mem::take(&mut m.successors);

    for chain in post_chains.into_iter().rev() {
        let chain_entry = fresh_label(next_label);
        match chain {
            PostChain::NegFields(neg_fields) => {
                emit_neg_fields_chain(chain_entry, current_succs, neg_fields, out, next_label);
            }
            PostChain::PostEffects(effects) => {
                emit_effects_chain_to_succs(chain_entry, current_succs, effects, out, next_label);
            }
            PostChain::Successors(succs) => {
                // The cascade holds the *overflow* (lowest-priority) successors. It is an
                // additional branch, so append its entry after the kept successors rather
                // than replacing them — replacing silently drops the kept successors.
                emit_successors_cascade(chain_entry, succs, out, next_label);
                current_succs.push(chain_entry);
                continue;
            }
        }
        current_succs = vec![chain_entry];
    }

    m.successors = current_succs;
    out.push(m.into());
}

fn drain_post_chains(m: &mut MatchIR) -> Vec<PostChain> {
    let mut post_chains = Vec::new();

    if m.neg_fields.len() > MAX_NEG_FIELDS {
        let overflow = m.neg_fields.drain(MAX_NEG_FIELDS..).collect();
        post_chains.push(PostChain::NegFields(overflow));
    }

    if m.post_effects.len() > MAX_POST_EFFECTS {
        let overflow = m.post_effects.drain(MAX_POST_EFFECTS..).collect();
        post_chains.push(PostChain::PostEffects(overflow));
    }

    let succ_budget = successor_budget(m);
    if m.successors.len() > succ_budget {
        let overflow = m.successors.drain(succ_budget - 1..).collect();
        post_chains.push(PostChain::Successors(overflow));
    }

    post_chains
}

fn successor_budget(m: &MatchIR) -> usize {
    // Successors share the 28-slot Match64 payload with the match's own retained
    // pre/neg/post effects and predicate (`MatchIR::resolve` panics on a combined
    // overflow). Budget the successor split against those other slots — not the bare
    // 28 — so the kept successors plus the cascade entry appended below land exactly at
    // the limit, never one over. (`other_slots` ≤ 7+7+7+2, so the budget stays ≥ 5.)
    let predicate_slots = if m.predicate.is_some() { 2 } else { 0 };
    let other_slots =
        m.pre_effects.len() + m.neg_fields.len() + m.post_effects.len() + predicate_slots;
    MAX_MATCH_PAYLOAD_SLOTS - other_slots
}

fn emit_effects_chain(
    entry: Label,
    exit: Label,
    mut effects: Vec<EffectIR>,
    out: &mut Vec<InstructionIR>,
    next_label: &mut u32,
) {
    if effects.is_empty() {
        out.push(MatchIR::epsilon(entry, exit).into());
        return;
    }

    if effects.len() <= MAX_PRE_EFFECTS {
        out.push(MatchIR::epsilon(entry, exit).pre_effects(effects).into());
        return;
    }

    let first_batch: Vec<_> = effects.drain(..MAX_PRE_EFFECTS).collect();
    let intermediate = fresh_label(next_label);
    out.push(
        MatchIR::epsilon(entry, intermediate)
            .pre_effects(first_batch)
            .into(),
    );
    emit_effects_chain(intermediate, exit, effects, out, next_label);
}

fn emit_neg_fields_chain(
    entry: Label,
    final_succs: Vec<Label>,
    mut neg_fields: Vec<u16>,
    out: &mut Vec<InstructionIR>,
    next_label: &mut u32,
) {
    if neg_fields.len() <= MAX_NEG_FIELDS {
        let mut m = MatchIR::terminal(entry).neg_fields(neg_fields);
        m.successors = final_succs;
        out.push(m.into());
        return;
    }

    let first_batch: Vec<_> = neg_fields.drain(..MAX_NEG_FIELDS).collect();
    let intermediate = fresh_label(next_label);
    out.push(
        MatchIR::terminal(entry)
            .neg_fields(first_batch)
            .next(intermediate)
            .into(),
    );
    emit_neg_fields_chain(intermediate, final_succs, neg_fields, out, next_label);
}

fn emit_effects_chain_to_succs(
    entry: Label,
    final_succs: Vec<Label>,
    mut effects: Vec<EffectIR>,
    out: &mut Vec<InstructionIR>,
    next_label: &mut u32,
) {
    if effects.len() <= MAX_POST_EFFECTS {
        let mut m = MatchIR::terminal(entry).post_effects(effects);
        m.successors = final_succs;
        out.push(m.into());
        return;
    }

    let first_batch: Vec<_> = effects.drain(..MAX_POST_EFFECTS).collect();
    let intermediate = fresh_label(next_label);
    out.push(
        MatchIR::terminal(entry)
            .post_effects(first_batch)
            .next(intermediate)
            .into(),
    );
    emit_effects_chain_to_succs(intermediate, final_succs, effects, out, next_label);
}

fn emit_successors_cascade(
    entry: Label,
    mut succs: Vec<Label>,
    out: &mut Vec<InstructionIR>,
    next_label: &mut u32,
) {
    if succs.len() <= MAX_MATCH_PAYLOAD_SLOTS {
        out.push(MatchIR::terminal(entry).successors(succs).into());
        return;
    }

    let overflow: Vec<_> = succs.drain(MAX_MATCH_PAYLOAD_SLOTS - 1..).collect();
    let intermediate = fresh_label(next_label);
    succs.push(intermediate);
    out.push(MatchIR::terminal(entry).successors(succs).into());
    emit_successors_cascade(intermediate, overflow, out, next_label);
}
