//! Trace query execution for debugging.

use std::path::PathBuf;

use plotnik_lib::{
    Colors, PrintTracer, RecordingTracer, RuntimeError, RuntimeLimitSpec, VM, Verbosity,
    materialize_verified,
};

use super::run_common::{self, ExecPlan, ExecRequest};
use super::runtime_report::render_runtime_error;
use crate::error::{CliError, CliResult};

const DEFAULT_MAX_RECORDS: usize = 65_536;

pub struct TraceArgs {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub source_path: Option<PathBuf>,
    pub source_text: Option<String>,
    pub lang: Option<String>,
    pub entry: Option<String>,
    pub verbosity: Verbosity,
    pub no_result: bool,
    pub limits: RuntimeLimitSpec,
    pub json: bool,
    pub color: bool,
}

pub fn run(args: TraceArgs) -> CliResult {
    let ExecPlan {
        module,
        entrypoint,
        tree,
        source_code,
    } = run_common::plan_exec(ExecRequest {
        query_path: args.query_path.as_deref(),
        query_text: args.query_text.as_deref(),
        source_path: args.source_path.as_deref(),
        source_text: args.source_text.as_deref(),
        lang: args.lang.as_deref(),
        entry: args.entry.as_deref(),
        color: args.color,
        inspection: args.json,
    })?;

    let vm = VM::builder(&source_code, &tree).limits(args.limits).build();

    if args.json {
        let mut tracer = RecordingTracer::new(&module, DEFAULT_MAX_RECORDS);
        let (result, stats) = vm.execute_with_stats(&module, &entrypoint, &mut tracer);
        let recording = tracer.finish();
        println!(
            "{}",
            serde_json::json!({
                "execution_trace": recording,
                "run_stats": stats,
            })
        );

        return match result {
            Ok(_) => Ok(()),
            Err(RuntimeError::NoMatch) => Err(CliError::No),
            Err(e) => {
                eprintln!("{}", render_runtime_error(&e, true));
                Err(CliError::FatalRendered)
            }
        };
    }

    let colors = Colors::new(args.color);
    let mut tracer = PrintTracer::builder(&source_code, &module)
        .verbosity(args.verbosity)
        .colored(args.color)
        .build();

    let effects = match vm.execute_with(&module, &entrypoint, &mut tracer) {
        Ok(effects) => {
            tracer.print();
            effects
        }
        Err(RuntimeError::NoMatch) => {
            tracer.print();
            eprintln!("no match");
            return Err(CliError::No);
        }
        Err(e) => {
            // `--json` structures only this error (on stderr); the trace itself is
            // always the textual trace format on stdout. trace is a debug tool, not
            // a JSON producer, so the two streams stay separate by design.
            tracer.print();
            eprintln!("{}", render_runtime_error(&e, args.json));
            return Err(CliError::FatalRendered);
        }
    };

    if args.no_result {
        return Ok(());
    }

    println!("{}---{}", colors.dim, colors.reset);
    let value = materialize_verified(
        &source_code,
        &module,
        &entrypoint,
        effects.as_slice(),
        colors,
    );

    let output = value.format(true, colors);
    println!("{}", output);

    Ok(())
}
