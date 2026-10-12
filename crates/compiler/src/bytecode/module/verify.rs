//! Projection from structurally valid bytecode into control-flow validation.

#[cfg(test)]
use std::cell::Cell;

use super::matcher_verify::{
    self, Call as VerifyCall, Effect as VerifyEffect, Entry as VerifyEntry,
    Instruction as VerifyInstruction, Match as VerifyMatch, Program as VerifyProgram,
    Return as VerifyReturn, VerifyError,
};
use super::{Instruction, Module, ModuleError};
use crate::bytecode::{CodeAddr, Effect, EffectKind, TypeDefKind, TypeKind};

#[cfg(test)]
thread_local! {
    static BODY_ANALYSES: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(super) fn reset_body_analyses() {
    BODY_ANALYSES.set(0);
}

#[cfg(test)]
pub(super) fn body_analyses() -> usize {
    BODY_ANALYSES.get()
}

#[cfg(test)]
fn record_body_analyses(count: usize) {
    BODY_ANALYSES.set(BODY_ANALYSES.get() + count);
}

pub(crate) fn validate(module: &Module) -> Result<(), ModuleError> {
    let mut instructions = Vec::with_capacity(module.header().instruction_word_count as usize);
    let mut address = CodeAddr::ZERO;
    while address.get() < module.header().instruction_word_count {
        let (instruction, words) = match module.decode_instruction(address) {
            Instruction::Match(matched) => {
                let effects = matched
                    .effects()
                    .map(|effect| project_effect(module, effect, address))
                    .collect::<Result<Vec<_>, _>>()?;
                let successors = matched
                    .successors()
                    .map(|successor| CodeAddr::from(u16::from(successor)))
                    .collect::<Vec<_>>();
                (
                    VerifyInstruction::Match(VerifyMatch::new(matched.nav, effects, successors)),
                    matched.word_count(),
                )
            }
            Instruction::Call(call) => {
                let returns = call
                    .returns()
                    .map(|continuation| CodeAddr::from(u16::from(continuation)))
                    .collect::<Vec<_>>();
                (
                    VerifyInstruction::Call(VerifyCall::new(
                        call.nav,
                        call.callee_contract(),
                        call.target,
                        returns,
                        call.consumed_mask(),
                    )),
                    call.word_count(),
                )
            }
            Instruction::Return(returned) => (
                VerifyInstruction::Return(VerifyReturn::new(returned.port, returned.contract)),
                1,
            ),
        };
        instructions.push((address, instruction));
        address = address
            .checked_add(words)
            .expect("validated instruction stream fits in u16 address space");
    }

    let entries = module
        .entry_points()
        .iter()
        .map(|entry| VerifyEntry::new(entry.target(), entry.boundary()))
        .collect();
    let program = VerifyProgram::new(instructions, entries).map_err(map_verify_error)?;
    let stats = matcher_verify::verify(&program).map_err(map_verify_error)?;
    #[cfg(test)]
    record_body_analyses(stats.body_analyses);
    #[cfg(not(test))]
    let _ = stats;
    Ok(())
}

fn project_effect(
    module: &Module,
    effect: Effect,
    address: CodeAddr,
) -> Result<VerifyEffect, ModuleError> {
    if effect.kind != EffectKind::VariantOpen {
        return Ok(VerifyEffect::new(effect.kind, effect.payload));
    }

    let types = module.types();
    let type_id = types.member_type_id(effect.payload);
    let Some(type_def) = types.get(type_id) else {
        return Err(ModuleError::EffectStackImbalance(address));
    };
    let has_no_payload = matches!(type_def.decode(), TypeDefKind::Primitive(TypeKind::NoValue));
    Ok(VerifyEffect::variant_open(effect.payload, has_no_payload))
}

fn map_verify_error(error: VerifyError<CodeAddr>) -> ModuleError {
    match error {
        VerifyError::Malformed { .. } => ModuleError::MalformedInstructionStream,
        VerifyError::EffectStack(address) => ModuleError::EffectStackImbalance(address),
        VerifyError::SpanStack(address) => ModuleError::SpanImbalance(address),
        VerifyError::StateBudget(address) => ModuleError::EffectStackBudget(address),
        VerifyError::CursorDepth { at, .. } => ModuleError::DepthImbalance(at),
        VerifyError::EmptyPathCursorRead(address) => ModuleError::EmptyPathCursorRead(address),
    }
}
