use std::io::IsTerminal;

use clap::ArgMatches;
use plotnik_compiler::{
    BytecodeConfig, Colors, PrintTracer, RuntimeError, VM, Verbosity, materialize_verified,
};

use super::{compile, inputs::Inputs, language, query_loader::load_query};
use crate::cli::{ColorChoice, runtime_limits};
use crate::error::{CliError, CliResult, write_stdout, writeln_stderr, writeln_stdout};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Run,
    Trace,
}

pub fn run(m: &ArgMatches, mode: Mode) -> CliResult {
    let inputs = Inputs::from_matches(m)?;
    let loaded = load_query(inputs.query_path, inputs.query_text)?;
    if loaded.sources.is_empty() {
        return Err(CliError::fatal("query cannot be empty"));
    }
    let source = inputs.source()?;
    let mut parser = language::load(m, inputs.source_path)?.parser()?;
    let color = ColorChoice::from_matches(m).should_colorize();
    let compiled = compile::compile_query(loaded.sources, parser.grammar(), color)?;
    let module = compile::emit_module(&compiled, BytecodeConfig::new(), color)?;
    let entry = m
        .get_one::<String>("entry")
        .map(String::as_str)
        .or_else(|| module.entry_point_names().last())
        .ok_or_else(|| CliError::fatal("query has no callable entry points"))?;
    let entry_point = module.entry_point(entry).ok_or_else(|| {
        CliError::fatal(format!(
            "unknown entry point {entry:?}; available: {}",
            module.entry_point_names().collect::<Vec<_>>().join(", ")
        ))
    })?;
    let tree = parser
        .parse(&source)
        .map_err(|error| CliError::fatal(error.to_string()))?;
    let vm = VM::builder(&source, &tree)
        .limits(runtime_limits(m))
        .build();

    let result = if mode == Mode::Trace {
        let verbosity = match m.get_count("verbose") {
            0 => Verbosity::Default,
            1 => Verbosity::Verbose,
            _ => Verbosity::VeryVerbose,
        };
        let mut tracer = PrintTracer::builder(&source, &module)
            .verbosity(verbosity)
            .colored(color)
            .build();
        let result = vm.execute_with(&module, &entry_point, &mut tracer);
        write_stdout(format_args!("{}", tracer.render()))?;
        result
    } else {
        vm.execute(&module, &entry_point)
    };
    let journal = match result {
        Ok(journal) => journal,
        Err(RuntimeError::NoMatch) => {
            writeln_stderr(format_args!("no match"))?;
            return Err(CliError::No);
        }
        Err(error) => return Err(CliError::fatal(error.to_string())),
    };
    if m.get_flag("no_result") {
        return Ok(());
    }
    let colors = Colors::new(color);
    let value = materialize_verified(
        &source,
        &module,
        &entry_point,
        journal.output_events(),
        colors,
    );
    let pretty = !m.get_flag("compact") && std::io::stdout().is_terminal();
    writeln_stdout(format_args!("{}", value.format(pretty, colors)))
}
