//! Always-on verification at the executor fork point.
//!
//! This boundary validates semantic-only metadata, then projects the lowered
//! NFA into the matcher verifier shared with the bytecode loader.

#[cfg(test)]
use std::cell::Cell;

use crate::bytecode::{CalleeContract, EffectKind};
use crate::compiler::analyze::result::{CaptureMemberKind, ResultSchema};
use crate::compiler::analyze::types::type_shape::CasePayload;
use crate::compiler::ids::ResultMemberId;
use crate::compiler::lower::ir::{
    CalleeEntryContract, DefSpecialization, EffectArg, EffectIR, InstructionIR, Label, NfaGraph,
    SemanticNfa,
};
use crate::matcher_verify::{
    self, BodyContract, Call as VerifyCall, Effect as VerifyEffect, EmptyPathCheck,
    Entry as VerifyEntry, Instruction as VerifyInstruction, Match as VerifyMatch,
    Program as VerifyProgram, Return as VerifyReturn, VerifyError,
};

pub(crate) const MAX_STATES: usize = u16::MAX as usize + 1;

#[cfg(test)]
thread_local! {
    static BODY_ANALYSES: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_body_analyses() {
    BODY_ANALYSES.set(0);
}

#[cfg(test)]
pub(crate) fn body_analyses() -> usize {
    BODY_ANALYSES.get()
}

#[cfg(test)]
fn record_body_analyses(count: usize) {
    BODY_ANALYSES.set(BODY_ANALYSES.get() + count);
}

#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum SemanticVerifyError {
    #[error("semantic matcher has {0} states (max {MAX_STATES})")]
    StateLimit(usize),
    #[error("semantic matcher has {0} entry points (max {max})", max = u16::MAX)]
    EntryPointLimit(usize),
    #[error("malformed semantic NFA: {0}")]
    Malformed(String),
    #[error("effect stack is imbalanced at state {0:?}")]
    EffectStack(Label),
    #[error("inspection span stack is imbalanced at state {0:?}")]
    SpanStack(Label),
    #[error("semantic verification state budget exceeded at state {0:?}")]
    StateBudget(Label),
    #[error("capture member reference is invalid at state {state:?}: {detail}")]
    CaptureMember { state: Label, detail: String },
    #[error("cursor depth is imbalanced: {0}")]
    CursorDepth(String),
    #[error("cursor-reading effect is reachable on an empty-match path at state {0:?}")]
    EmptyPathCursorRead(Label),
    #[error("native regex DFA compilation failed for `{pattern}`: {error}")]
    Regex { pattern: String, error: String },
}

impl SemanticVerifyError {
    pub(crate) fn is_query_rejection(&self) -> bool {
        matches!(
            self,
            Self::StateLimit(_)
                | Self::EntryPointLimit(_)
                | Self::StateBudget(_)
                | Self::Regex { .. }
        )
    }
}

pub(crate) fn verify(
    semantic: &SemanticNfa,
    schema: &ResultSchema<'_>,
) -> Result<(), SemanticVerifyError> {
    verify_state_count(semantic)?;

    let graph = semantic.raw();
    if graph.entry_points().len() > u16::MAX as usize {
        return Err(SemanticVerifyError::EntryPointLimit(
            graph.entry_points().len(),
        ));
    }
    verify_regexes(graph)?;

    let program = project_program(graph, schema)?;
    let stats =
        matcher_verify::verify(&program, EmptyPathCheck::Verify).map_err(map_verify_error)?;
    #[cfg(test)]
    record_body_analyses(stats.body_analyses);
    #[cfg(not(test))]
    let _ = stats;
    Ok(())
}

fn verify_regexes(graph: &NfaGraph) -> Result<(), SemanticVerifyError> {
    for instruction in graph.instructions() {
        let InstructionIR::Match(matched) = instruction else {
            continue;
        };
        let Some(predicate) = &matched.predicate else {
            continue;
        };
        let crate::compiler::lower::ir::PredicateValueIR::Regex(pattern) = &predicate.value else {
            continue;
        };
        let normalized = crate::compiler::regex::normalize(pattern);
        crate::compiler::regex::compile_native_dfa(&normalized).map_err(|error| {
            SemanticVerifyError::Regex {
                pattern: pattern.to_string(),
                error,
            }
        })?;
    }
    Ok(())
}

fn verify_state_count(semantic: &SemanticNfa) -> Result<(), SemanticVerifyError> {
    let count = semantic.raw().instructions().len();
    if count > MAX_STATES {
        return Err(SemanticVerifyError::StateLimit(count));
    }
    Ok(())
}

fn project_program(
    graph: &NfaGraph,
    schema: &ResultSchema<'_>,
) -> Result<VerifyProgram<Label>, SemanticVerifyError> {
    verify_public_entries(graph)?;

    let mut instructions = Vec::with_capacity(graph.instructions().len());
    for instruction in graph.instructions() {
        let projected = match instruction {
            InstructionIR::Match(matched) => {
                let effects = matched
                    .effects
                    .iter()
                    .map(|effect| project_effect(effect, matched.label, schema))
                    .collect::<Result<Vec<_>, _>>()?;
                VerifyInstruction::Match(VerifyMatch::new(
                    matched.nav,
                    effects,
                    matched.successors.clone(),
                ))
            }
            InstructionIR::Call(call) => {
                let specialization = call_specialization(graph, call)?;
                VerifyInstruction::Call(VerifyCall::new(
                    call.entry.nav(),
                    project_contract(call.entry.target_contract()),
                    call.target,
                    call.returns.clone(),
                    consumed_mask(specialization),
                ))
            }
            InstructionIR::Return(returned) => VerifyInstruction::Return(VerifyReturn::new(
                returned.port,
                project_contract(returned.entry),
            )),
        };
        instructions.push((instruction.label(), projected));
    }

    let entries = graph
        .entry_points()
        .values()
        .map(|entry| VerifyEntry::new(entry.target, entry.boundary))
        .collect();
    let roots = graph.def_entries.iter().map(|(specialization, &entry)| {
        (
            entry,
            BodyContract::new(
                project_contract(specialization.entry_contract()),
                specialization.ports().len(),
                consumed_mask(specialization),
            ),
        )
    });

    VerifyProgram::new(instructions, entries, roots).map_err(map_verify_error)
}

fn verify_public_entries(graph: &NfaGraph) -> Result<(), SemanticVerifyError> {
    for (&def_id, entry) in graph.entry_points() {
        let specialization = DefSpecialization::ordinary(def_id);
        if graph.def_entries.get(&specialization) != Some(&entry.target) {
            return Err(SemanticVerifyError::Malformed(format!(
                "entry point for {def_id:?} does not target its ordinary definition body"
            )));
        }
    }
    Ok(())
}

fn call_specialization<'a>(
    graph: &'a NfaGraph,
    call: &crate::compiler::lower::ir::CallIR,
) -> Result<&'a DefSpecialization, SemanticVerifyError> {
    let Some(specialization) = graph.specialization_for_entry(call.target) else {
        return Err(SemanticVerifyError::Malformed(format!(
            "call {:?} targets a non-definition entry {:?}",
            call.label, call.target
        )));
    };
    Ok(specialization)
}

fn consumed_mask(specialization: &DefSpecialization) -> u8 {
    specialization
        .ports()
        .ports()
        .iter()
        .enumerate()
        .fold(0, |mask, (index, port)| {
            mask | if port.consumed() { 1 << index } else { 0 }
        })
}

fn project_contract(contract: CalleeEntryContract) -> CalleeContract {
    match contract {
        CalleeEntryContract::CallerOwned => CalleeContract::CallerOwned,
        CalleeEntryContract::CalleeOwned { obligation } => CalleeContract::CalleeOwned {
            nav: obligation.navigation().authored(),
            node_field: obligation.field(),
        },
    }
}

fn project_effect(
    effect: &EffectIR,
    label: Label,
    schema: &ResultSchema<'_>,
) -> Result<VerifyEffect, SemanticVerifyError> {
    match effect.kind() {
        EffectKind::RecordSet => {
            let member = member(effect, label)?;
            if !matches!(
                member_descriptor(schema, member, label)?.kind,
                CaptureMemberKind::Field(_)
            ) {
                return Err(capture_error(
                    label,
                    "RecordSet references a non-field member",
                ));
            }
            Ok(VerifyEffect::new(effect.kind(), member.index()))
        }
        EffectKind::VariantOpen => {
            let member = member(effect, label)?;
            let CaptureMemberKind::Case(payload) = member_descriptor(schema, member, label)?.kind
            else {
                return Err(capture_error(
                    label,
                    "VariantOpen does not reference a variant case",
                ));
            };
            Ok(VerifyEffect::variant_open(
                member.index(),
                payload == CasePayload::NoPayload,
            ))
        }
        kind => Ok(VerifyEffect::new(kind, literal(effect, label)?)),
    }
}

fn member_descriptor(
    schema: &ResultSchema<'_>,
    member: ResultMemberId,
    label: Label,
) -> Result<crate::compiler::analyze::result::CaptureMember, SemanticVerifyError> {
    schema
        .layout()
        .member(member)
        .copied()
        .ok_or_else(|| capture_error(label, "member id is out of bounds"))
}

fn literal(effect: &EffectIR, label: Label) -> Result<usize, SemanticVerifyError> {
    match effect.argument() {
        EffectArg::Literal(value) => Ok(*value),
        EffectArg::Member(_) => Err(capture_error(
            label,
            "literal effect uses a member argument",
        )),
    }
}

fn member(effect: &EffectIR, label: Label) -> Result<ResultMemberId, SemanticVerifyError> {
    match effect.argument() {
        EffectArg::Member(member) => Ok(*member),
        EffectArg::Literal(_) => Err(capture_error(
            label,
            "member effect uses a literal argument",
        )),
    }
}

fn capture_error(label: Label, detail: impl Into<String>) -> SemanticVerifyError {
    SemanticVerifyError::CaptureMember {
        state: label,
        detail: detail.into(),
    }
}

fn map_verify_error(error: VerifyError<Label>) -> SemanticVerifyError {
    match error {
        VerifyError::Malformed { at, detail } => {
            let at = at.map_or_else(String::new, |label| format!(" at {label:?}"));
            SemanticVerifyError::Malformed(format!("{detail}{at}"))
        }
        VerifyError::EffectStack(label) => SemanticVerifyError::EffectStack(label),
        VerifyError::SpanStack(label) => SemanticVerifyError::SpanStack(label),
        VerifyError::StateBudget(label) => SemanticVerifyError::StateBudget(label),
        VerifyError::CursorDepth { at, detail } => {
            SemanticVerifyError::CursorDepth(format!("{detail} at {at:?}"))
        }
        VerifyError::EmptyPathCursorRead(label) => SemanticVerifyError::EmptyPathCursorRead(label),
    }
}
