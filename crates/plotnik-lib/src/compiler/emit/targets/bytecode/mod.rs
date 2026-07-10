//! VM bytecode target.

mod instructions;
mod layout;
mod layout_map;
mod module;
pub(in crate::compiler) mod regex_table;
pub(in crate::compiler) mod string_table;
pub(in crate::compiler) mod tables;
pub(in crate::compiler) mod type_table;

#[cfg(test)]
mod regex_table_tests;

use crate::compiler::analyze::AnalysisArtifacts;
use crate::compiler::analyze::output::OutputSchema;
use crate::compiler::lower::ir::LoweredNfa;

use self::instructions::emit_instructions;
use self::layout::compute_layout;
use self::module::EmitPipeline;
use self::regex_table::build_regex_table;
use self::string_table::seed_string_table;
use self::tables::{ConstantPool, EmitError};
use self::type_table::build_type_table;

pub(in crate::compiler) fn emit(
    input: AnalysisArtifacts<'_>,
    lowered_ir: &LoweredNfa,
) -> Result<Vec<u8>, EmitError> {
    let nfa = lowered_ir.raw();
    let schema = OutputSchema::from_artifacts(input)?;
    let strings = seed_string_table(nfa)?;
    let (types, strings) = build_type_table(&schema, strings)?;
    let layout = compute_layout(nfa)?;
    let mut pipeline = EmitPipeline::new(input, nfa, strings, types, layout, schema.layout());
    let tables = pipeline.build_tables()?;
    let regexes = build_regex_table(nfa, pipeline.strings())?;
    let pool = ConstantPool::new(
        pipeline.types(),
        pipeline.strings(),
        &regexes,
        schema.layout(),
    );
    let transitions = emit_instructions(nfa.instructions(), pipeline.layout(), pool)?;
    pipeline.write_module(pool, &tables, &transitions)
}
