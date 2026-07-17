use plotnik_lib::grammar::{DEFAULT_WIDTH, DumpOptions};

use crate::error::{CliError, CliResult, write_stdout, writeln_stdout};
use crate::language_registry;

pub fn run_list() -> CliResult {
    for lang in language_registry::all() {
        let aliases: Vec<_> = lang.aliases().iter().skip(1).copied().collect();
        if aliases.is_empty() {
            writeln_stdout(format_args!("{}", lang.name()))?;
        } else {
            writeln_stdout(format_args!("{} ({})", lang.name(), aliases.join(", ")))?;
        }
    }

    Ok(())
}

pub fn run_dump(lang_name: &str, legend: bool, json: bool, width: Option<usize>) -> CliResult {
    let lang = super::lang_resolver::resolve_lang_name(lang_name)?;

    if json {
        // The machine escape hatch: hand back the grammar's own source format.
        let raw = lang
            .raw()
            .to_json()
            .map_err(|e| CliError::fatal(e.to_string()))?;
        writeln_stdout(format_args!("{raw}"))?;
        return Ok(());
    }

    let options = DumpOptions {
        legend,
        width: width.unwrap_or(DEFAULT_WIDTH),
    };
    write_stdout(format_args!("{}", lang.grammar().tree().dump(&options)))?;

    Ok(())
}
