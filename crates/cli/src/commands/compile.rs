use plotnik_compiler::bytecode::Module;
use plotnik_compiler::grammar::Grammar;
use plotnik_compiler::{
    BytecodeConfig, CompiledQuery, Diagnostics, QueryBuilder, SourceCollection, SourceMap,
};

use crate::error::{CliError, CliResult, writeln_stderr};

pub fn compile_query(
    sources: SourceCollection,
    grammar: &Grammar,
    color: bool,
) -> Result<CompiledQuery, CliError> {
    let compiled = QueryBuilder::from_sources(sources)
        .compile(grammar)
        .map_err(|e| CliError::fatal(e.to_string()))?;

    report_diagnostics(compiled.diagnostics(), compiled.source_map(), color)?;
    Ok(compiled)
}

pub fn report_diagnostics(
    diagnostics: &Diagnostics,
    sources: &SourceMap,
    color: bool,
) -> CliResult {
    if !diagnostics.is_empty() {
        writeln_stderr(format_args!(
            "{}",
            diagnostics.render_colored(sources, color)
        ))?;
    }

    if diagnostics.has_errors() {
        return Err(CliError::No);
    }

    Ok(())
}

pub fn compile_module(
    sources: SourceCollection,
    grammar: &Grammar,
    color: bool,
) -> Result<Module, CliError> {
    let compiled = compile_query(sources, grammar, color)?;
    emit_module(&compiled, color)
}

pub fn emit_module(compiled: &CompiledQuery, color: bool) -> Result<Module, CliError> {
    let emission = compiled
        .emit(BytecodeConfig::new())
        .map_err(|error| CliError::fatal(error.to_string()))?;
    report_diagnostics(emission.diagnostics(), compiled.source_map(), color)?;
    emission
        .into_artifact()
        .ok_or_else(|| CliError::fatal("bytecode emission produced no module"))
}
