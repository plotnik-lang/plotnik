#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Regex-table emission phase: compile predicate regexes to sparse DFAs.
//!
//! Compilation lives here so the regex engine stays out of `core`; the table
//! that accumulates the compiled bytes is a core type.

use regex_automata::dfa::dense;

use crate::bytecode::StringId;
use crate::compiler::emit::tables::{EmitError, RegexId, RegexTableBuilder, StringTableBuilder};
use crate::compiler::lower::ir::{InstructionIR, NfaGraph, PredicateValueIR};

/// Compile every regex predicate into the regex table, resolving each pattern's
/// StringId from the finished string table. Reads the string table; interns
/// nothing into it.
pub fn build_regex_table(
    nfa: &NfaGraph,
    strings: &StringTableBuilder,
) -> Result<RegexTableBuilder, EmitError> {
    let mut regexes = RegexTableBuilder::new();
    intern_regex_predicates(nfa.instructions(), strings, &mut regexes)?;
    regexes.validate()?;
    Ok(regexes)
}

fn intern_regex_predicates(
    instructions: &[InstructionIR],
    strings: &StringTableBuilder,
    regexes: &mut RegexTableBuilder,
) -> Result<(), EmitError> {
    for instr in instructions {
        if let InstructionIR::Match(m) = instr
            && let Some(pred) = &m.predicate
            && let PredicateValueIR::Regex(pattern) = &pred.value
        {
            let string_id = strings
                .lookup_str(pattern.as_ref())
                .expect("regex predicate string must be interned before regex emission");
            intern(regexes, pattern.as_ref(), string_id)?;
        }
    }
    Ok(())
}

/// Compile `pattern` to a sparse DFA and store it under `string_id`, returning
/// its regex ID. Deduplicates by StringId, so a repeated pattern compiles once.
pub(super) fn intern(
    regexes: &mut RegexTableBuilder,
    pattern: &str,
    string_id: StringId,
) -> Result<RegexId, EmitError> {
    if let Some(id) = regexes.lookup(string_id) {
        return Ok(id);
    }

    let bytes =
        compile_dfa_bytes(pattern).map_err(|e| EmitError::RegexCompile(pattern.to_string(), e))?;
    regexes.push_dfa(string_id, bytes)
}

/// Compile `pattern` to serialized sparse-DFA bytes. The one compilation path
/// shared by the bytecode emitter and the Rust code generator, so a DFA
/// embedded in generated code is bit-identical to the wire's.
pub(in crate::compiler) fn compile_dfa_bytes(pattern: &str) -> Result<Vec<u8>, String> {
    let dense = dense::DFA::builder()
        .configure(
            dense::DFA::config()
                .start_kind(regex_automata::dfa::StartKind::Unanchored)
                .minimize(true),
        )
        .build(pattern)
        .map_err(|e| e.to_string())?;

    let sparse = dense.to_sparse().map_err(|e| e.to_string())?;
    Ok(sparse.to_bytes_little_endian())
}
