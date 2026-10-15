use crate::bytecode::StringId;
use crate::compiler::emit::targets::bytecode::tables::{
    EmitError, RegexId, RegexTableBuilder, StringTableBuilder,
};
use crate::compiler::lower::ir::{InstructionIR, NfaGraph, PredicateValueIR};
use crate::compiler::regex::{compile_native_dfa, normalize};

pub fn build_regex_table(
    nfa: &NfaGraph,
    strings: &StringTableBuilder,
) -> Result<RegexTableBuilder, EmitError> {
    let mut regexes = RegexTableBuilder::new();
    intern_regex_predicates(nfa.instructions(), strings, &mut regexes)?;
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

pub(super) fn intern(
    regexes: &mut RegexTableBuilder,
    pattern: &str,
    string_id: StringId,
) -> Result<RegexId, EmitError> {
    if let Some(id) = regexes.lookup(string_id) {
        return Ok(id);
    }

    let normalized = normalize(pattern);
    let bytes = compile_native_dfa(&normalized)
        .map_err(|error| EmitError::RegexCompile(pattern.to_string(), error))?;
    regexes.push_dfa(string_id, bytes)
}
