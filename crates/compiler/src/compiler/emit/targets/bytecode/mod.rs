//! VM bytecode target.

mod instructions;
mod layout;
mod layout_map;
mod module;
pub(in crate::compiler) mod regex_table;
pub(in crate::compiler) mod string_table;
pub(in crate::compiler) mod tables;
pub(in crate::compiler) mod type_table;

use crate::compiler::analyze::AnalysisArtifacts;
use crate::compiler::analyze::result::ResultSchema;
use crate::compiler::lower::ir::LoweredNfa;

use self::instructions::emit_instructions;
use self::module::EmitPipeline;
use self::regex_table::build_regex_table;
use self::tables::{ConstantPool, EmitError};

pub(in crate::compiler) fn emit(
    input: AnalysisArtifacts<'_>,
    schema: &ResultSchema<'_>,
    lowered_ir: &LoweredNfa,
) -> Result<Vec<u8>, EmitError> {
    let nfa = lowered_ir.raw();
    let mut pipeline = EmitPipeline::prepare(input, nfa, schema)?;
    let tables = pipeline.build_tables()?;
    let regexes = build_regex_table(nfa, pipeline.strings())?;
    let pool = ConstantPool::new(pipeline.types(), pipeline.strings(), &regexes);
    let instructions = emit_instructions(nfa, pipeline.layout(), pool)?;
    pipeline.write_module(pool, &tables, &instructions)
}
