//! Shared target-neutral compile and bytecode emission paths.

use plotnik_lib::bytecode::Module;
use plotnik_lib::grammar::Grammar;
use plotnik_lib::{BytecodeConfig, CompiledQuery, QueryBuilder, SourceMap};

use crate::error::CliError;
use crate::language_registry::Lang;

/// Parse, analyze, link, lower, and validate target-neutral compiler IR.
pub fn compile_query(
    sources: SourceMap,
    lang: &Lang,
    color: bool,
) -> Result<CompiledQuery, CliError> {
    compile_query_with_grammar(sources, lang.grammar(), color)
}

pub fn compile_query_with_grammar(
    sources: SourceMap,
    grammar: &Grammar,
    color: bool,
) -> Result<CompiledQuery, CliError> {
    let compiled = QueryBuilder::new(sources)
        .compile(grammar)
        .map_err(|e| CliError::fatal(e.to_string()))?;

    let diagnostics = compiled.diagnostics();
    if diagnostics.has_errors() {
        eprint!(
            "{}",
            diagnostics.render_colored(compiled.source_map(), color)
        );
        return Err(CliError::FatalRendered);
    }

    Ok(compiled)
}

pub fn compile_module(sources: SourceMap, lang: &Lang, color: bool) -> Result<Module, CliError> {
    let compiled = compile_query(sources, lang, color)?;
    emit_module(&compiled, BytecodeConfig::new(), color)
}

pub fn emit_module(
    compiled: &CompiledQuery,
    config: BytecodeConfig,
    color: bool,
) -> Result<Module, CliError> {
    let emission = compiled
        .emit(config)
        .map_err(|error| CliError::fatal(error.to_string()))?;
    let has_errors = emission.diagnostics().has_errors();
    if !emission.diagnostics().is_empty() {
        eprint!(
            "{}",
            emission
                .diagnostics()
                .render_colored(compiled.source_map(), color)
        );
    }
    if has_errors {
        return Err(CliError::No);
    }
    emission
        .into_artifact()
        .ok_or_else(|| CliError::fatal("bytecode emission produced no module"))
}
