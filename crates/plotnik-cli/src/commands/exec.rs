//! Execute a query and output JSON result.

use std::path::PathBuf;

use plotnik_lib::Colors;
use plotnik_lib::engine::{Materializer, RuntimeError, VM, ValueMaterializer, debug_verify_type};

use super::run_common::{self, PreparedQuery, QueryInput};

pub struct ExecArgs {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub source_path: Option<PathBuf>,
    pub source_text: Option<String>,
    pub lang: Option<String>,
    pub pretty: bool,
    pub entry: Option<String>,
    pub color: bool,
}

pub fn run(args: ExecArgs) {
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

    let vm = VM::builder(&source_code, &tree).trivia_types(trivia_types).build();
    let effects = match vm.execute(&module, 0, &entrypoint) {
        Ok(effects) => effects,
        Err(RuntimeError::NoMatch) => {
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("runtime error: {}", e);
            std::process::exit(2);
        }
    };

    let materializer = ValueMaterializer::new(&source_code, module.types(), module.strings());
    let value = materializer.materialize(effects.as_slice(), entrypoint.result_type());

    let colors = Colors::new(args.color);

    // Debug-only: verify output matches declared type
    debug_verify_type(&value, entrypoint.result_type(), &module, colors);

    let output = value.format(args.pretty, colors);
    println!("{}", output);
}
