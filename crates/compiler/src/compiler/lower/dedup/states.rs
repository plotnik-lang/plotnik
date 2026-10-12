//! State deduplication: hash-cons structurally identical instructions.
//!
//! Thompson construction freely duplicates small states — a search-nav
//! quantifier's first and repeat iterations each emit a position search whose
//! `navigate` and `retry` states are byte-identical wildcard `Next` transitions into
//! the same `try` state (#475). Two instructions that perform the same
//! operation and continue to the same successors are bisimilar: no execution
//! can tell which one it is in, so every reference to the duplicate is
//! redirected to the first occurrence and the duplicate is dropped.
//!
//! Merging runs to a fixed point because collapsing one pair can make its
//! predecessors identical in turn. Each round tries two successor keyings:
//!
//! - **Self-normalized**: a state's edge to itself is a sentinel, so twin
//!   retry loops (`A → [.., A]` vs `B → [.., B]`) merge. This is the only
//!   graph-shape normalization; mutually-recursive twins (`A → B`, `B → A`)
//!   are left alone — hash-consing under-approximates bisimilarity, which is
//!   enough for construction-made duplicates.
//! - **Raw**: catches the copy that jumps *into* a twin's loop
//!   (`B → [.., A]` merging into the self-looping `A → [.., A]`), which
//!   self-normalization renders differently.
//!
//! Duplicate entries within one successor list also collapse (keep first):
//! successor attempts restart from the same origin state, so a repeated label
//! is a provably identical retry. Merges create these (`→ [a, b]` where `b`
//! merged into `a`), and removing them re-enables further merges.
//!
//! `Return` is deliberately never merged: every `Return` is behaviorally
//! identical, so keying them would fold all defs' accept states into one
//! instruction across definition boundaries — an 8-byte saving per def that
//! would cost each def its own accept state in dumps and layout locality.
//!
//! This pass cannot run under `verify::run_verified`: the debug fingerprint
//! walks every path and cuts cycles by per-path visited *labels*, so merging
//! twin states on a loop shifts the cut one hop earlier (the path revisits the
//! representative where it previously reached the distinct twin first). The
//! recorded paths differ even though each is an op-for-op prefix of the same
//! infinite unrolling — a bisimulation quotient preserves path semantics but
//! not label-cut path *sets*. Structural soundness and scope balance are
//! re-checked by `verify_constructed` after the pass instead.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use crate::bytecode::{Nav, NodeKindConstraint};
use crate::core::NodeFieldId;

use crate::compiler::lower::ir::{
    CallEntry, EffectIR, InstructionIR, Label, NfaGraph, PredicateIR,
};

pub fn dedup_states(nfa: &mut NfaGraph) {
    loop {
        dedup_successors(&mut nfa.instructions);

        let mut remap = plan_merges(&nfa.instructions, SuccNorm::SelfLoop);
        if remap.is_empty() {
            remap = plan_merges(&nfa.instructions, SuccNorm::Raw);
        }
        if remap.is_empty() {
            break;
        }

        apply_remap(nfa, &remap);
        nfa.instructions
            .retain(|instr| !remap.contains_key(&instr.label()));
    }
}

/// How successor labels enter the dedup key. See the module docs for why both
/// are needed.
#[derive(Clone, Copy)]
enum SuccNorm {
    SelfLoop,
    Raw,
}

/// A successor slot in a dedup key.
#[derive(PartialEq, Eq, Hash)]
enum SuccKey {
    SelfLoop,
    Other(Label),
}

/// Everything observable about an instruction except its own label.
#[derive(PartialEq, Eq, Hash)]
enum StateKey {
    Match {
        nav: Nav,
        node_kind: NodeKindConstraint,
        node_field: Option<NodeFieldId>,
        effects: Vec<EffectIR>,
        neg_fields: Vec<NodeFieldId>,
        predicate: Option<PredicateIR>,
        successors: Vec<SuccKey>,
    },
    Call {
        entry: CallEntry,
        returns: Vec<SuccKey>,
        target: Label,
    },
}

impl StateKey {
    fn of(instr: &InstructionIR, norm: SuccNorm) -> Option<Self> {
        match instr {
            InstructionIR::Match(m) => Some(Self::Match {
                nav: m.nav,
                node_kind: m.node_kind,
                node_field: m.node_field,
                effects: m.effects.clone(),
                neg_fields: m.neg_fields.clone(),
                predicate: m.predicate.clone(),
                successors: m
                    .successors
                    .iter()
                    .map(|&s| match norm {
                        SuccNorm::SelfLoop if s == m.label => SuccKey::SelfLoop,
                        _ => SuccKey::Other(s),
                    })
                    .collect(),
            }),
            InstructionIR::Call(c) => Some(Self::Call {
                entry: c.entry,
                returns: c
                    .returns
                    .iter()
                    .map(|&s| match norm {
                        SuccNorm::SelfLoop if s == c.label => SuccKey::SelfLoop,
                        _ => SuccKey::Other(s),
                    })
                    .collect(),
                target: c.target,
            }),
            InstructionIR::Return(_) => None,
        }
    }
}

/// One merge round: group instructions by structural key. Within a group the
/// first occurrence (instruction order, which layout follows) survives; the
/// rest map to it. Representatives are never themselves remapped in a round,
/// so the map is single-level.
fn plan_merges(instructions: &[InstructionIR], norm: SuccNorm) -> HashMap<Label, Label> {
    let mut representatives: HashMap<StateKey, Label> = HashMap::new();
    let mut remap: HashMap<Label, Label> = HashMap::new();

    for instr in instructions {
        let Some(key) = StateKey::of(instr, norm) else {
            continue;
        };
        match representatives.entry(key) {
            Entry::Vacant(slot) => {
                slot.insert(instr.label());
            }
            Entry::Occupied(rep) => {
                remap.insert(instr.label(), *rep.get());
            }
        }
    }

    remap
}

/// Rewrite every reference in the graph through `remap`: successor lists, call
/// continuations and targets, and entry points themselves.
fn apply_remap(nfa: &mut NfaGraph, remap: &HashMap<Label, Label>) {
    if let Some((&label, &representative)) = remap
        .iter()
        .find(|(_, representative)| remap.contains_key(representative))
    {
        panic!(
            "NFA state deduplication produced a chained remap: {label:?} maps to \
             {representative:?}, which is itself remapped; remaps must point directly to a final \
             representative"
        );
    }
    let resolve = |label: Label| remap.get(&label).copied().unwrap_or(label);

    for instr in &mut nfa.instructions {
        match instr {
            InstructionIR::Match(m) => {
                for succ in &mut m.successors {
                    *succ = resolve(*succ);
                }
            }
            InstructionIR::Call(c) => {
                c.remap_returns(resolve);
                c.target = resolve(c.target);
            }
            InstructionIR::Return(_) => {}
        }
    }

    for entry in nfa.def_entries.values_mut() {
        *entry = resolve(*entry);
    }
    for entry in nfa.entry_points.values_mut() {
        entry.target = resolve(entry.target);
    }
}

/// Drop repeated labels within each successor list, keeping the first
/// (highest-priority) occurrence.
fn dedup_successors(instructions: &mut [InstructionIR]) {
    for instr in instructions {
        let InstructionIR::Match(m) = instr else {
            continue;
        };
        if m.successors.len() < 2 {
            continue;
        }
        let mut seen = std::collections::HashSet::with_capacity(m.successors.len());
        m.successors.retain(|&s| seen.insert(s));
    }
}
