use std::io::{IsTerminal, Read as _};

use clap::ArgMatches;
use plotnik_compiler::bytecode::{Entrypoint, Module};
use plotnik_compiler::{NoopTracer, PrintTracer, QueryBuilder, SourceMap, Tracer, VM, Verbosity};
use plotnik_runtime::Text;
use plotnik_runtime::json::{Palette, Style};

use super::{
    compile,
    inputs::{Inputs, load_source},
    language::LanguageOptions,
    query_loader::{QuerySources, load_query},
};
use crate::cli::{ColorChoice, runtime_limits};
use crate::error::{CliError, CliResult, writeln_stderr, writeln_stdout};

pub fn run(m: &ArgMatches, command: &str) -> CliResult {
    let inputs = Inputs::from_matches(m)?;
    let mut loaded = match inputs.query_path {
        Some(path) => load_query(path)?,
        None => QuerySources {
            sources: SourceMap::new(),
            is_directory: false,
        },
    };

    let expression = if inputs.entry.is_none() {
        let mut text = String::new();
        std::io::stdin()
            .read_to_string(&mut text)
            .map_err(|error| CliError::fatal(format!("failed to read stdin: {error}")))?;
        Some(loaded.sources.add_stdin(&text))
    } else {
        None
    };

    let mut builder = QueryBuilder::from_sources(loaded.into_collection());
    if let Some(expression) = expression {
        builder = builder.with_expression(expression);
    }

    let source = load_source(inputs.source_path)?;
    let color = ColorChoice::from_matches(m).should_colorize();
    let artifacts = LanguageOptions::from_matches(m).load(Some(inputs.source_path))?;
    let compiled = builder
        .compile_dynamic(&artifacts.grammar)
        .map_err(|error| CliError::fatal(error.to_string()))?;
    compile::report_diagnostics(compiled.diagnostics(), compiled.source_map(), color)?;

    let module = compile::emit_module(&compiled, color)?;
    let entrypoint = match inputs.entry {
        None => module
            .expression_entrypoint()
            .expect("valid expression emits an entrypoint"),
        Some(entry) => module.entrypoint(entry).ok_or_else(|| {
            CliError::fatal(format!(
                "unknown entrypoint {entry:?}; available: {}",
                module.entrypoint_names().collect::<Vec<_>>().join(", ")
            ))
        })?,
    };
    let mut parser = artifacts.parser()?;
    let tree = parser
        .parse(&source)
        .map_err(|error| CliError::fatal(error.to_string()))?;
    let vm = VM::builder(&source, &tree)
        .limits(runtime_limits(m))
        .build();
    let style = Style {
        pretty: !m.get_flag("compact") && std::io::stdout().is_terminal(),
        palette: if color { Palette::ANSI } else { Palette::PLAIN },
    };
    let result = if m.get_flag("trace") {
        let verbosity = match m.get_count("verbose") {
            0 => Verbosity::Default,
            1 => Verbosity::Verbose,
            _ => Verbosity::VeryVerbose,
        };
        let mut tracer = PrintTracer::builder(&source, &module)
            .verbosity(verbosity)
            .colored(color)
            .build();
        let result = execute(vm, &module, entrypoint, command, &mut tracer, style);
        match tracer.render() {
            Ok(trace) => writeln_stderr(format_args!("{trace}"))?,
            Err(error) if result.is_ok() => return Err(CliError::fatal(error.to_string())),
            Err(_) => {}
        }
        result
    } else {
        execute(vm, &module, entrypoint, command, &mut NoopTracer, style)
    };
    let Some(value) = result? else {
        writeln_stderr(format_args!("no match"))?;
        return Err(CliError::No);
    };

    writeln_stdout(format_args!("{value}"))
}

fn execute<'s, 't, T: Tracer>(
    vm: VM<'s, 't>,
    module: &'s Module,
    entrypoint: Entrypoint,
    command: &str,
    tracer: &mut T,
    style: Style,
) -> Result<Option<Text>, CliError> {
    let account = vm
        .account()
        .map_err(|error| CliError::fatal(error.to_string()))?
        .clone();
    let mut output = Text::new(&account);
    if command == "find-all" {
        output
            .push(&mut account.meter(), "[")
            .map_err(|error| CliError::fatal(error.to_string()))?;
        for (index, result) in vm.traverse_with(module, entrypoint, tracer).enumerate() {
            let result = result.map_err(|error| CliError::fatal(error.to_string()))?;
            if index > 0 {
                output
                    .push(&mut account.meter(), ",")
                    .map_err(|error| CliError::fatal(error.to_string()))?;
            }
            result.write_json(style, &mut output).map_err(|error| {
                CliError::fatal(
                    output
                        .error()
                        .map_or_else(|| error.to_string(), |error| error.to_string()),
                )
            })?;
        }
        output
            .push(&mut account.meter(), "]")
            .map_err(|error| CliError::fatal(error.to_string()))?;
        return Ok(Some(output));
    }

    let result = match command {
        "exec" => vm.apply_with(module, &entrypoint, tracer),
        "find" => vm
            .traverse_with(module, entrypoint, tracer)
            .next()
            .transpose(),
        _ => unreachable!("unexpected execution command: {command:?}"),
    }
    .map_err(|error| CliError::fatal(error.to_string()))?;
    let Some(result) = result else {
        return Ok(None);
    };
    result.write_json(style, &mut output).map_err(|error| {
        CliError::fatal(
            output
                .error()
                .map_or_else(|| error.to_string(), |error| error.to_string()),
        )
    })?;
    Ok(Some(output))
}
