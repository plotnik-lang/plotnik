//! Pre-decoded instruction stream, built once at module load.
//!
//! The VM's dispatch loop previously re-parsed instruction bytes on every
//! visit. Load-time validation proves the stream well-formed, so the same walk
//! now also materializes fixed-size structs the loop can index directly; the
//! byte-level decoders remain the single source of truth and feed this build.

use crate::bytecode::{
    BYTECODE_WORD_SIZE, CallOwnership, CodeAddr, Effect, Nav, NodeKindConstraint, PredicateOp,
    SuccessorAddr,
};
use crate::core::NodeFieldId;
use plotnik_rt::PortId;

use super::super::instructions::header_byte;
use super::Instruction;

/// One pre-decoded bytecode word. Load-time validation proves every jump target
/// and successor lands on an instruction start; `Interior` makes a violation
/// fail loudly in release builds too.
#[derive(Clone, Copy, Debug)]
pub(crate) enum DecodedInstr {
    Match(DecodedMatch),
    Call(DecodedCall),
    Return(PortId),
    Interior,
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
    pub(crate) ownership: CallOwnership,
    pub(crate) nav: Nav,
    pub(crate) node_field: Option<NodeFieldId>,
    pub(crate) target: SuccessorAddr,
    returns_base: u32,
    returns_len: u8,
}

impl DecodedCall {
    pub(crate) fn caller_owned(self) -> bool {
        self.ownership == CallOwnership::Caller
    }
}

/// The whole pre-decoded instruction stream plus the side pools its
/// variable-length lists point into.
#[derive(Debug, Default)]
pub(crate) struct DecodedProgram {
    words: Vec<DecodedInstr>,
    effects: Vec<Effect>,
    neg_fields: Vec<NodeFieldId>,
    /// Encoded successor addresses (validated instruction starts, never 0-as-terminal:
    /// terminality is `successors(..).is_empty()`).
    successors: Vec<SuccessorAddr>,
}

impl DecodedProgram {
    #[inline]
    pub(crate) fn instruction_at(&self, addr: CodeAddr) -> DecodedInstr {
        let instruction = self.words[u16::from(addr) as usize];
        assert!(
            !matches!(instruction, DecodedInstr::Interior),
            "decoded bytecode lookup addressed interior word {addr:?} of a multi-word instruction; \
             validation must restrict control flow to instruction starts"
        );
        instruction
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
    pub(crate) fn successors(&self, m: &DecodedMatch) -> &[SuccessorAddr] {
        &self.successors[m.succ_base as usize..][..m.succ_len as usize]
    }

    #[inline]
    pub(crate) fn call_returns(&self, call: &DecodedCall) -> &[SuccessorAddr] {
        &self.successors[call.returns_base as usize..][..call.returns_len as usize]
    }

    #[inline]
    pub(crate) fn call_return(&self, call: &DecodedCall, port: PortId) -> SuccessorAddr {
        self.call_returns(call)[port.index()]
    }
}

/// Walk the validated instruction stream and pre-decode every instruction.
/// `instructions` is a whole number of bytecode words.
pub(crate) fn build(instructions: &[u8]) -> DecodedProgram {
    let word_count = instructions.len() / BYTECODE_WORD_SIZE;
    let mut program = DecodedProgram::default();
    program.words.reserve(word_count);

    let mut word_addr = 0usize;
    while word_addr < word_count {
        let bytes = &instructions[word_addr * BYTECODE_WORD_SIZE..];
        let opcode = header_byte::opcode(bytes[0]).expect("validated opcode");
        match Instruction::from_bytes(bytes) {
            Instruction::Match(m) => {
                let effects_base = pool_base(program.effects.len());
                program.effects.extend(m.effects());
                let neg_base = pool_base(program.neg_fields.len());
                program.neg_fields.extend(m.neg_fields());
                let succ_base = pool_base(program.successors.len());
                program.successors.extend(m.successors());

                let predicate = m.predicate().map(|p| DecodedPredicate {
                    op: PredicateOp::from_byte(p.op),
                    is_regex: p.is_regex,
                    value_ref: p.value_ref,
                });

                program.words.push(DecodedInstr::Match(DecodedMatch {
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
                // Interior words of a multi-word Match are never addressed.
                for _ in 1..opcode.word_count() {
                    program.words.push(DecodedInstr::Interior);
                }
                word_addr += opcode.word_count() as usize;
            }
            Instruction::Call(c) => {
                let returns_base = pool_base(program.successors.len());
                program.successors.extend(c.returns());
                program.words.push(DecodedInstr::Call(DecodedCall {
                    ownership: c.ownership,
                    nav: c.nav,
                    node_field: c.node_field,
                    target: c.target,
                    returns_base,
                    returns_len: c.arity() as u8,
                }));
                for _ in 1..opcode.word_count() {
                    program.words.push(DecodedInstr::Interior);
                }
                word_addr += opcode.word_count() as usize;
            }
            Instruction::Return(return_) => {
                program.words.push(DecodedInstr::Return(return_.port));
                word_addr += 1;
            }
        }
    }
    program
}

fn pool_base(len: usize) -> u32 {
    u32::try_from(len).expect("decoded payload pool exceeds u32")
}
