//! Pre-decoded instruction stream, built once at module load.
//!
//! The VM's dispatch loop previously re-parsed instruction bytes on every
//! visit. Load-time validation proves the stream well-formed, so the same walk
//! now also materializes fixed-size structs the loop can index directly; the
//! byte-level decoders remain the single source of truth and feed this build.

use crate::bytecode::{Effect, Nav, NodeKindConstraint, PredicateOp, STEP_SIZE};
use crate::core::NodeFieldId;

use super::super::instructions::header_byte;
use super::Instruction;

/// One pre-decoded step. Interior slots of a multi-step `Match` hold `Return`
/// placeholders that are never addressed: load-time validation proves every
/// jump target and successor lands on an instruction start.
#[derive(Clone, Copy, Debug)]
pub(crate) enum DecodedInstr {
    Match(DecodedMatch),
    Call(DecodedCall),
    Return,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct DecodedMatch {
    pub(crate) nav: Nav,
    pub(crate) node_kind: NodeKindConstraint,
    pub(crate) node_field: Option<NodeFieldId>,
    /// Node must be a tree-sitter MISSING node — the `(MISSING …)` constraint.
    pub(crate) missing: bool,
    pub(crate) predicate: Option<DecodedPredicate>,
    effects_base: u32,
    neg_base: u32,
    succ_base: u32,
    effects_len: u8,
    neg_len: u8,
    succ_len: u8,
}

impl DecodedMatch {
    #[inline]
    pub(crate) fn is_epsilon(&self) -> bool {
        self.nav == Nav::Epsilon
    }
}

/// Predicate with the operator already decoded, so evaluation does no byte
/// re-interpretation.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DecodedPredicate {
    pub(crate) op: PredicateOp,
    pub(crate) is_regex: bool,
    pub(crate) value_ref: u16,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct DecodedCall {
    pub(crate) nav: Nav,
    pub(crate) node_field: Option<NodeFieldId>,
    pub(crate) next: u16,
    pub(crate) target: u16,
}

/// The whole pre-decoded transitions section plus the side pools its
/// variable-length lists point into.
#[derive(Debug, Default)]
pub(crate) struct DecodedProgram {
    steps: Vec<DecodedInstr>,
    effects: Vec<Effect>,
    neg_fields: Vec<NodeFieldId>,
    /// Raw step addresses (validated instruction starts, never 0-as-terminal:
    /// terminality is `successors(..).is_empty()`).
    successors: Vec<u16>,
}

impl DecodedProgram {
    #[inline]
    pub(crate) fn step(&self, step: u16) -> DecodedInstr {
        self.steps[step as usize]
    }

    #[inline]
    pub(crate) fn effects(&self, m: &DecodedMatch) -> &[Effect] {
        &self.effects[m.effects_base as usize..][..m.effects_len as usize]
    }

    #[inline]
    pub(crate) fn neg_fields(&self, m: &DecodedMatch) -> &[NodeFieldId] {
        &self.neg_fields[m.neg_base as usize..][..m.neg_len as usize]
    }

    #[inline]
    pub(crate) fn successors(&self, m: &DecodedMatch) -> &[u16] {
        &self.successors[m.succ_base as usize..][..m.succ_len as usize]
    }
}

/// Walk the validated transitions section and pre-decode every instruction.
/// `transitions` is the section's byte slice, a whole number of 8-byte steps.
pub(crate) fn build(transitions: &[u8]) -> DecodedProgram {
    let step_count = transitions.len() / STEP_SIZE;
    let mut program = DecodedProgram::default();
    program.steps.reserve(step_count);

    let mut step = 0usize;
    while step < step_count {
        let bytes = &transitions[step * STEP_SIZE..];
        let opcode = header_byte::opcode(bytes[0]).expect("validated opcode");
        match Instruction::from_bytes(bytes) {
            Instruction::Match(m) => {
                let effects_base = pool_base(program.effects.len());
                program.effects.extend(m.effects());
                let neg_base = pool_base(program.neg_fields.len());
                program.neg_fields.extend(m.neg_fields());
                let succ_base = pool_base(program.successors.len());
                program.successors.extend(m.successors().map(u16::from));

                let predicate = m.predicate().map(|p| DecodedPredicate {
                    op: PredicateOp::from_byte(p.op),
                    is_regex: p.is_regex,
                    value_ref: p.value_ref,
                });

                program.steps.push(DecodedInstr::Match(DecodedMatch {
                    nav: m.nav,
                    node_kind: m.node_kind,
                    node_field: m.node_field,
                    missing: m.missing(),
                    predicate,
                    effects_base,
                    neg_base,
                    succ_base,
                    effects_len: (program.effects.len() - effects_base as usize) as u8,
                    neg_len: (program.neg_fields.len() - neg_base as usize) as u8,
                    succ_len: (program.successors.len() - succ_base as usize) as u8,
                }));
                // Interior slots of a multi-step Match are never addressed.
                for _ in 1..opcode.step_count() {
                    program.steps.push(DecodedInstr::Return);
                }
                step += opcode.step_count() as usize;
            }
            Instruction::Call(c) => {
                program.steps.push(DecodedInstr::Call(DecodedCall {
                    nav: c.nav,
                    node_field: c.node_field,
                    next: u16::from(c.next),
                    target: u16::from(c.target),
                }));
                step += 1;
            }
            Instruction::Return(_) => {
                program.steps.push(DecodedInstr::Return);
                step += 1;
            }
        }
    }
    program
}

fn pool_base(len: usize) -> u32 {
    u32::try_from(len).expect("decoded payload pool exceeds u32")
}
