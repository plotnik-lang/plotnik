use std::collections::{HashMap, HashSet};

use crate::bytecode::{
    BYTECODE_WORD_SIZE, CallOwnership, CodeAddr, Effect, Nav, PredicateOp, SuccessorAddr,
    SwitchEntry,
};
use crate::core::{NodeFieldId, NodeKindId};
use plotnik_runtime::PortId;

use super::super::instructions::header_byte;
use super::{Instruction, Module, ModuleError};

#[derive(Clone, Copy, Debug)]
pub(crate) enum DecodedInstr {
    Switch(DecodedSwitch),
    Match(DecodedMatch),
    Call(DecodedCall),
    Return(PortId),
    Interior,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct DecodedSwitch {
    base: u32,
    len: u16,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct DecodedMatch {
    pub(crate) nav: Nav,
    pub(crate) kind: Option<NodeKindId>,
    pub(crate) candidate: DecodedCandidate,
    effects_base: u32,
    succ_base: u32,
    effects_len: u8,
    succ_len: u8,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct DecodedCandidate {
    pub(crate) named: Option<bool>,
    pub(crate) node_field: Option<NodeFieldId>,
    pub(crate) missing: bool,
    pub(crate) predicate: Option<DecodedPredicate>,
    neg_base: u32,
    neg_len: u8,
}

impl DecodedMatch {
    #[inline]
    pub(crate) fn is_epsilon(&self) -> bool {
        self.nav == Nav::Epsilon
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct DecodedPredicate {
    pub(crate) op: PredicateOp,
    pub(crate) value_ref: u16,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct DecodedCall {
    pub(crate) ownership: CallOwnership,
    pub(crate) nav: Nav,
    pub(crate) node_field: Option<NodeFieldId>,
    pub(crate) target: CodeAddr,
    returns_base: u32,
    returns_len: u8,
}

impl DecodedCall {
    pub(crate) fn caller_owned(self) -> bool {
        self.ownership == CallOwnership::Caller
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct DecodedSwitchEntry {
    pub(crate) kind: NodeKindId,
    action: u16,
}

#[derive(Debug)]
pub(crate) struct DecodedAction {
    pub(crate) entry: CodeAddr,
    pub(crate) matched_at: CodeAddr,
    pub(crate) matched: DecodedMatch,
    pub(crate) prefix: Vec<DecodedPrefix>,
}

#[derive(Debug)]
pub(crate) struct DecodedPrefix {
    pub(crate) ip: CodeAddr,
    effects_base: u32,
    effects_len: u8,
}

#[derive(Debug, Default)]
pub(crate) struct DecodedProgram {
    words: Vec<DecodedInstr>,
    effects: Vec<Effect>,
    neg_fields: Vec<NodeFieldId>,
    successors: Vec<SuccessorAddr>,
    switch_entries: Vec<DecodedSwitchEntry>,
    actions: Vec<DecodedAction>,
}

impl DecodedProgram {
    #[inline]
    pub(crate) fn switch_entries(&self, switch: &DecodedSwitch) -> &[DecodedSwitchEntry] {
        &self.switch_entries[switch.base as usize..][..usize::from(switch.len)]
    }

    #[inline]
    pub(crate) fn switch_action(&self, entry: &DecodedSwitchEntry) -> &DecodedAction {
        &self.actions[usize::from(entry.action)]
    }

    pub(crate) fn prefix_effects(&self, prefix: &DecodedPrefix) -> &[Effect] {
        &self.effects[prefix.effects_base as usize..][..prefix.effects_len as usize]
    }

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
    pub(crate) fn neg_fields(&self, candidate: &DecodedCandidate) -> &[NodeFieldId] {
        &self.neg_fields[candidate.neg_base as usize..][..candidate.neg_len as usize]
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

pub(crate) fn build(module: &Module) -> Result<DecodedProgram, ModuleError> {
    let instructions = module.instructions_slice();
    let word_count = instructions.len() / BYTECODE_WORD_SIZE;
    let mut program = DecodedProgram::default();
    program.words.reserve(word_count);

    let mut word_addr = 0usize;
    while word_addr < word_count {
        let bytes = &instructions[word_addr * BYTECODE_WORD_SIZE..];
        let opcode = header_byte::opcode(bytes[0]).expect("validated opcode");
        match module.decode_instruction(CodeAddr::from(word_addr as u16)) {
            Instruction::Switch { header, .. } => {
                program.words.push(DecodedInstr::Switch(DecodedSwitch {
                    base: header.table_offset / SwitchEntry::SIZE as u32,
                    len: header.case_count,
                }));
                word_addr += 1;
            }
            Instruction::Match(m) => {
                let effects_base = pool_base(program.effects.len());
                program.effects.extend(m.effects());
                let neg_base = pool_base(program.neg_fields.len());
                program.neg_fields.extend(m.neg_fields());
                let succ_base = pool_base(program.successors.len());
                program.successors.extend(m.successors());

                let predicate = m.predicate().map(|p| DecodedPredicate {
                    op: p.op,
                    value_ref: p.value_ref,
                });

                program.words.push(DecodedInstr::Match(DecodedMatch {
                    nav: m.nav,
                    kind: m.node_kind.kind_id(),
                    candidate: DecodedCandidate {
                        named: (!m.node_kind.is_any()).then_some(m.node_kind.is_named()),
                        node_field: m.node_field,
                        missing: m.missing(),
                        predicate,
                        neg_base,
                        neg_len: (program.neg_fields.len() - neg_base as usize) as u8,
                    },
                    effects_base,
                    succ_base,
                    effects_len: (program.effects.len() - effects_base as usize) as u8,
                    succ_len: (program.successors.len() - succ_base as usize) as u8,
                }));
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
    let mut actions = HashMap::new();
    for entry in module.switch_tables().iter() {
        let target = CodeAddr::from(u16::from(entry.target));
        let action = if let Some(&index) = actions.get(&(target, entry.kind)) {
            index
        } else {
            let action = program.selected_entry(target, entry.kind)?;
            let index = u16::try_from(program.actions.len())
                .expect("selected entries are bounded by instruction count");
            program.actions.push(action);
            actions.insert((target, entry.kind), index);
            index
        };
        program.switch_entries.push(DecodedSwitchEntry {
            kind: entry.kind,
            action,
        });
    }
    Ok(program)
}

impl DecodedProgram {
    fn selected_entry(
        &self,
        entry: CodeAddr,
        kind: NodeKindId,
    ) -> Result<DecodedAction, ModuleError> {
        let mut ip = entry;
        let mut visited = HashSet::new();
        let mut prefix = Vec::new();
        loop {
            if !visited.insert(ip) {
                return Err(ModuleError::MalformedSwitchTable);
            }

            let DecodedInstr::Match(matched) = self.instruction_at(ip) else {
                return Err(ModuleError::MalformedSwitchTable);
            };
            if !matched.is_epsilon() {
                if matched.nav != Nav::StayExact || matched.kind != Some(kind) {
                    return Err(ModuleError::MalformedSwitchTable);
                }

                return Ok(DecodedAction {
                    entry,
                    matched_at: ip,
                    matched,
                    prefix,
                });
            }

            let [next] = self.successors(&matched) else {
                return Err(ModuleError::MalformedSwitchTable);
            };
            prefix.push(DecodedPrefix {
                ip,
                effects_base: matched.effects_base,
                effects_len: matched.effects_len,
            });
            ip = CodeAddr::from(u16::from(*next));
        }
    }
}

fn pool_base(len: usize) -> u32 {
    u32::try_from(len).expect("decoded payload pool exceeds u32")
}
