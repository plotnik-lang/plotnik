use std::io::IsTerminal;

use clap::ArgMatches;
use plotnik_lib::{
    BytecodeConfig, BytecodeInspection, Colors, PrintTracer, RuntimeError, TraceRecorder,
    TypeScriptCodegenConfig, VM, Verbosity, extract_result_provenance, materialize_verified,
};
use serde_json::json;

use super::{compile, inputs::Inputs, language, query_loader::load_query};
use crate::cli::{ColorChoice, runtime_limits};
use crate::error::{CliError, CliResult, write_stdout, writeln_stderr, writeln_stdout};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Run,
    Trace,
    Inspect,
}

pub fn run(m: &ArgMatches, mode: Mode) -> CliResult {
    let inputs = Inputs::from_matches(m)?;
    let loaded = load_query(inputs.query_path, inputs.query_text)?;
    if loaded.sources.is_empty() {
        return Err(CliError::fatal("query cannot be empty"));
    }
    let source = inputs.source()?;
    let mut parser = language::load(m, inputs.source_path)?.parser();
    let color = ColorChoice::from_matches(m).should_colorize();
    let compiled = compile::compile_query(loaded.sources, parser.grammar(), color)?;
    let inspection = mode == Mode::Inspect || m.get_flag("json");
    let config = if inspection {
        BytecodeConfig::new().inspection(BytecodeInspection::Spans)
    } else {
        BytecodeConfig::new()
    };
    let module = compile::emit_module(&compiled, config, color)?;
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

    if mode == Mode::Inspect || m.get_flag("json") {
        let record_trace =
            mode == Mode::Trace || (mode == Mode::Inspect && m.get_count("verbose") > 0);
        let mut tracer = TraceRecorder::new(&module, if record_trace { 65_536 } else { 0 });
        let (result, stats) = vm.execute_with_stats(&module, &entry_point, &mut tracer);
        let trace = record_trace.then(|| tracer.finish());
        let mut bundle = json!({ "run_stats": stats });
        if mode == Mode::Inspect {
            let types = compiled
                .emit_types(TypeScriptCodegenConfig::new())
                .map_err(|error| CliError::fatal(error.to_string()))?
                .into_artifact()
                .ok_or_else(|| CliError::fatal("type emission produced no declarations"))?;
            let (declarations, bindings) = types.into_parts();
            bundle["typescript_declarations"] = json!(declarations);
            bundle["typescript_bindings"] = json!(bindings);
            bundle["diagnostics"] = json!(compiled.diagnostics().to_wire(compiled.source_map()));
            bundle["entry_points"] = json!(module.entry_point_names().collect::<Vec<_>>());
        }
        if let Some(trace) = trace {
            bundle["execution_trace"] = json!(trace);
        }
        let exit = match result {
            Ok(journal) => {
                if !m.get_flag("no_result") {
                    let value = materialize_verified(
                        &source,
                        &module,
                        &entry_point,
                        journal.output_events(),
                        Colors::new(false),
                    );
                    bundle["result"] = json!(value);
                    if inspection {
                        bundle["result_provenance"] =
                            json!(extract_result_provenance(&journal, &module));
                    }
                }
                Ok(())
            }
            Err(RuntimeError::NoMatch) => {
                bundle["result"] = serde_json::Value::Null;
                bundle["error"] = json!("no match");
                Err(CliError::No)
            }
            Err(error) => {
                bundle["error"] = json!(error.to_string());
                Err(CliError::fatal(error.to_string()))
            }
        };
        writeln_stdout(format_args!("{bundle}"))?;
        return exit;
    }

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
