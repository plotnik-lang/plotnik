//! Shared compile path for run, dump, and infer.
//!
//! All three funnel through `check_compile` — the same full-pipeline validation
//! `check` runs — so a query `check` rejects fails here too, as a rendered
//! diagnostic rather than a panic on `Module::load`.

use plotnik_lib::QueryBuilder;
use plotnik_lib::SourceMap;
use plotnik_lib::bytecode::Module;

use crate::error::CliError;
use plotnik::language_registry::Lang;

/// Parse, analyze, link, validate (full `check_compile`), emit, load.
///
/// `check_compile` already proves emit+load succeed, so the final load can only
/// fail on a genuine bug; it is surfaced as a clean error, never a panic.
pub fn compile_module(sources: SourceMap, lang: &Lang, color: bool) -> Result<Module, CliError> {
    let linked = QueryBuilder::new(sources)
        .parse()
        .map_err(|e| CliError::fatal(e.to_string()))?
        .analyze()
        .link(lang.grammar());

    let diagnostics = linked.check_compile();
    if diagnostics.has_errors() {
        eprint!("{}", diagnostics.render_colored(linked.source_map(), color));
        return Err(CliError::FatalRendered);
    }

    let bytecode = linked.emit().map_err(|e| CliError::fatal(e.to_string()))?;
    Module::load(&bytecode).map_err(|e| CliError::fatal(format!("bytecode rejected: {e}")))
}
