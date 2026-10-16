use plotnik_compiler::bytecode::Module;
use plotnik_compiler::grammar::Grammar;
use plotnik_compiler::{BytecodeConfig, CompiledQuery, QueryBuilder, SourceCollection};

use crate::error::{CliError, CliResult, write_stderr};

pub fn compile_query(
    sources: SourceCollection,
    grammar: &Grammar,
    color: bool,
) -> Result<CompiledQuery, CliError> {
    let compiled = QueryBuilder::from_sources(sources)
        .compile(grammar)
        .map_err(|e| CliError::fatal(e.to_string()))?;

    report_errors(&compiled, color)?;
    Ok(compiled)
}

pub fn report_errors(compiled: &CompiledQuery, color: bool) -> CliResult {
    let diagnostics = compiled.diagnostics();
    if !diagnostics.has_errors() {
        return Ok(());
    }

    write_stderr(format_args!(
        "{}",
        diagnostics.render_colored(compiled.source_map(), color)
    ))?;
    Err(CliError::No)
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
    let has_errors = emission.diagnostics().has_errors();
    if !emission.diagnostics().is_empty() {
        write_stderr(format_args!(
            "{}",
            emission
                .diagnostics()
                .render_colored(compiled.source_map(), color)
        ))?;
    }
    if has_errors {
        return Err(CliError::No);
    }
    emission
        .into_artifact()
        .ok_or_else(|| CliError::fatal("bytecode emission produced no module"))
}
