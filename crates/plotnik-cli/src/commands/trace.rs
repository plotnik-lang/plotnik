//! Trace query execution for debugging.

use std::path::PathBuf;

use plotnik_lib::Colors;
use plotnik_lib::engine::{PrintTracer, RuntimeError, VM, Verbosity, materialize_verified};

use super::run_common::{self, ExecPlan, ExecRequest};
use crate::error::{CliError, CliResult};

pub struct TraceArgs {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub source_path: Option<PathBuf>,
    pub source_text: Option<String>,
    pub lang: Option<String>,
    pub entry: Option<String>,
    pub verbosity: Verbosity,
    pub no_result: bool,
    pub fuel: u32,
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
    })?;

    let vm = VM::builder(&source_code, &tree)
        .step_budget(args.fuel)
        .build();
    let colors = Colors::new(args.color);
    let mut tracer = PrintTracer::builder(&source_code, &module)
        .verbosity(args.verbosity)
        .colored(args.color)
        .build();

    let effects = match vm.execute_with(&module, 0, &entrypoint, &mut tracer) {
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
            tracer.print();
            return Err(CliError::fatal(format!("runtime error: {}", e)));
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
