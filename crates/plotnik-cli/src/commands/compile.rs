//! Shared compile path for run, dump, and infer.
//!
//! All three funnel through `check_compile` — the same full-pipeline validation
//! `check` runs — so a query `check` rejects fails here too, as a rendered
//! diagnostic rather than a panic on `Module::load`.

use plotnik_lib::bytecode::Module;
use plotnik_lib::{CompiledQuery, QueryBuilder, SourceMap};

use crate::error::CliError;
use crate::language_registry::Lang;

/// Parse, analyze, link, validate (full `check_compile`), emit, load.
///
/// `check_compile` already proves emit+load succeed, so the final load can only
/// fail on a genuine bug; it is surfaced as a clean error, never a panic.
pub fn compile_query(
    sources: SourceMap,
    lang: &Lang,
    color: bool,
) -> Result<CompiledQuery, CliError> {
    let compiled = QueryBuilder::new(sources)
        .compile(lang.grammar())
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
    compiled
        .into_module()
        .ok_or_else(|| CliError::fatal("compile produced no module"))
}
