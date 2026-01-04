//! Trace query execution for debugging.

use std::path::PathBuf;

use plotnik_lib::engine::{
    debug_verify_type, FuelLimits, Materializer, PrintTracer, RuntimeError, ValueMaterializer,
    Verbosity, VM,
};
use plotnik_lib::Colors;

use super::run_common::{self, PreparedQuery, QueryInput};

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

pub fn run(args: TraceArgs) {
    let PreparedQuery {
        module,
        entrypoint,
        tree,
        trivia_types,
        source_code,
    } = run_common::prepare_query(QueryInput {
        query_path: args.query_path.as_deref(),
        query_text: args.query_text.as_deref(),
        source_path: args.source_path.as_deref(),
        source_text: args.source_text.as_deref(),
        lang: args.lang.as_deref(),
        entry: args.entry.as_deref(),
        color: args.color,
    });

    let limits = FuelLimits {
        exec_fuel: args.fuel,
        ..Default::default()
    };
    let vm = VM::new(&tree, trivia_types, limits);
    let colors = Colors::new(args.color);
    let mut tracer = PrintTracer::new(&source_code, &module, args.verbosity, colors);

    let effects = match vm.execute_with(&module, &entrypoint, &mut tracer) {
        Ok(effects) => {
            tracer.print();
            effects
        }
        Err(RuntimeError::NoMatch) => {
            tracer.print();
            std::process::exit(1);
        }
        Err(e) => {
            tracer.print();
            eprintln!("runtime error: {}", e);
            std::process::exit(2);
        }
    };

    if args.no_result {
        return;
    }

    println!("{}---{}", colors.dim, colors.reset);
    let materializer = ValueMaterializer::new(&source_code, module.types(), module.strings());
    let value = materializer.materialize(effects.as_slice(), entrypoint.result_type);

    // Debug-only: verify output matches declared type
    debug_verify_type(&value, entrypoint.result_type, &module, colors);

    let output = value.format(true, colors);
    println!("{}", output);
}
