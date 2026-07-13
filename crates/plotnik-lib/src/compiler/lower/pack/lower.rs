//! Lowering pass: transforms unconstrained IR into bytecode-compatible form.
//!
//! This pass handles bytecode encoding constraints by splitting oversized
//! instructions into cascading epsilon chains:
//! - effects > 15 → epsilon chain after match
//! - neg_fields > 7 → epsilon chain for overflow checks
//! - successors > 28 → cascading epsilon forks

use crate::bytecode::{MAX_EFFECTS, MAX_MATCH_PAYLOAD_SLOTS, MAX_NEG_FIELDS};
use crate::core::NodeFieldId;

use crate::compiler::lower::ir::{EffectIR, InstructionIR, Label, MatchIR, NfaGraph};

enum PostChain {
    NegFields(Vec<NodeFieldId>),
    Effects(Vec<EffectIR>),
    Successors(Vec<Label>),
}

pub fn pack_instructions(result: &mut NfaGraph) {
    let next_label = result
        .instructions
        .iter()
        .map(|i| i.label().0)
        .max()
        .unwrap_or(0)
        + 1;

    // Earlier passes may have deleted the highest-numbered labels, so ids from
    // `next_label` up get *reused* for cascade states. Drop the dead labels'
    // origins so a reused id reads as a wire artifact (no origin), not as the
    // dead label's definition.
    result.label_origins.truncate(next_label as usize);

    let mut emitter = Emitter::new(result.instructions.len(), next_label);

    for instr in result.instructions.drain(..) {
        let InstructionIR::Match(m) = instr else {
            emitter.push(instr);
            continue;
        };
        emitter.lower_match(m);
    }

    result.instructions = emitter.finish();
}

struct Emitter {
    out: Vec<InstructionIR>,
    next_label: u32,
}

impl Emitter {
    fn new(capacity: usize, next_label: u32) -> Self {
        Self {
            out: Vec::with_capacity(capacity),
            next_label,
        }
    }

    fn push(&mut self, instr: InstructionIR) {
        self.out.push(instr);
    }

    fn finish(self) -> Vec<InstructionIR> {
        self.out
    }

    fn fresh_label(&mut self) -> Label {
        let label = Label(self.next_label);
        self.next_label += 1;
        label
    }

    fn lower_match(&mut self, mut m: MatchIR) {
        let post_chains = drain_post_chains(&mut m);
        if post_chains.is_empty() {
            self.push(m.into());
            return;
        }

        let mut current_succs = std::mem::take(&mut m.successors);

        for chain in post_chains.into_iter().rev() {
            let chain_entry = self.fresh_label();
            match chain {
                PostChain::NegFields(neg_fields) => {
                    self.emit_neg_fields_chain(chain_entry, current_succs, neg_fields);
                }
                PostChain::Effects(effects) => {
                    self.emit_effects_chain_to_succs(chain_entry, current_succs, effects);
                }
                PostChain::Successors(succs) => {
                    // The cascade holds the *overflow* (lowest-priority) successors. It is an
                    // additional successor, so append its entry after the kept successors rather
                    // than replacing them — replacing silently drops the kept successors.
                    self.emit_successors_cascade(chain_entry, succs);
                    current_succs.push(chain_entry);
                    continue;
                }
            }
            current_succs = vec![chain_entry];
        }

        m.successors = current_succs;
        self.push(m.into());
    }

    fn emit_neg_fields_chain(
        &mut self,
        entry: Label,
        final_succs: Vec<Label>,
        mut neg_fields: Vec<NodeFieldId>,
    ) {
        if neg_fields.len() <= MAX_NEG_FIELDS {
            let mut m = MatchIR::terminal(entry).neg_fields(neg_fields);
            m.successors = final_succs;
            self.push(m.into());
            return;
        }

        let first_batch: Vec<_> = neg_fields.drain(..MAX_NEG_FIELDS).collect();
        let intermediate = self.fresh_label();
        self.push(
            MatchIR::terminal(entry)
                .neg_fields(first_batch)
                .next(intermediate)
                .into(),
        );
        self.emit_neg_fields_chain(intermediate, final_succs, neg_fields);
    }

    fn emit_effects_chain_to_succs(
        &mut self,
        entry: Label,
        final_succs: Vec<Label>,
        mut effects: Vec<EffectIR>,
    ) {
        if effects.len() <= MAX_EFFECTS {
            let mut m = MatchIR::terminal(entry).append_effects(effects);
            m.successors = final_succs;
            self.push(m.into());
            return;
        }

        let first_batch: Vec<_> = effects.drain(..MAX_EFFECTS).collect();
        let intermediate = self.fresh_label();
        self.push(
            MatchIR::terminal(entry)
                .append_effects(first_batch)
                .next(intermediate)
                .into(),
        );
        self.emit_effects_chain_to_succs(intermediate, final_succs, effects);
    }

    fn emit_successors_cascade(&mut self, entry: Label, mut succs: Vec<Label>) {
        if succs.len() <= MAX_MATCH_PAYLOAD_SLOTS {
            self.push(MatchIR::terminal(entry).successors(succs).into());
            return;
        }

        let overflow: Vec<_> = succs.drain(MAX_MATCH_PAYLOAD_SLOTS - 1..).collect();
        let intermediate = self.fresh_label();
        succs.push(intermediate);
        self.push(MatchIR::terminal(entry).successors(succs).into());
        self.emit_successors_cascade(intermediate, overflow);
    }
}

fn drain_post_chains(m: &mut MatchIR) -> Vec<PostChain> {
    let mut post_chains = Vec::new();

    if m.neg_fields.len() > MAX_NEG_FIELDS {
        let overflow = m.neg_fields.drain(MAX_NEG_FIELDS..).collect();
        post_chains.push(PostChain::NegFields(overflow));
    }

    if m.effects.len() > MAX_EFFECTS {
        let overflow = m.effects.drain(MAX_EFFECTS..).collect();
        post_chains.push(PostChain::Effects(overflow));
    }

    let succ_budget = successor_budget(m);
    if m.successors.len() > succ_budget {
        let overflow = m.successors.drain(succ_budget - 1..).collect();
        post_chains.push(PostChain::Successors(overflow));
    }

    post_chains
}

fn successor_budget(m: &MatchIR) -> usize {
    // Successors share the 28-slot Match64 payload with the match's retained
    // effects, neg fields, and predicate (`MatchIR::resolve` panics on a combined
    // overflow). Budget the successor split against those other slots — not the bare
    // 28 — so the kept successors plus the cascade entry appended below land exactly at
    // the limit, never one over. (`other_slots` ≤ 15+7+2, so the budget stays ≥ 4.)
    let predicate_slots = if m.predicate.is_some() { 2 } else { 0 };
    let other_slots = m.effects.len() + m.neg_fields.len() + predicate_slots;
    MAX_MATCH_PAYLOAD_SLOTS - other_slots
}
