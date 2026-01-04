//! Execute a query and output JSON result.

use std::path::PathBuf;

use plotnik_lib::bytecode::Module;
use plotnik_lib::emit::emit_linked;
use plotnik_lib::Colors;
use plotnik_lib::QueryBuilder;

use plotnik_lib::engine::{
    debug_verify_type, FuelLimits, Materializer, RuntimeError, ValueMaterializer, VM,
};

use super::query_loader::load_query_source;
use super::run_common;

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
    if let Err(msg) = run_common::validate(
        args.query_text.is_some() || args.query_path.is_some(),
        args.source_text.is_some() || args.source_path.is_some(),
        args.source_text.is_some(),
        args.lang.is_some(),
    ) {
        eprintln!("error: {}", msg);
        std::process::exit(1);
    }

    let source_map = match load_query_source(args.query_path.as_deref(), args.query_text.as_deref())
    {
        Ok(map) => map,
        Err(msg) => {
            eprintln!("error: {}", msg);
            std::process::exit(1);
        }
    };

    if source_map.is_empty() {
        eprintln!("error: query cannot be empty");
        std::process::exit(1);
    }

    let source_code = run_common::load_source(
        args.source_text.as_deref(),
        args.source_path.as_deref(),
        args.query_path.as_deref(),
    );
    let lang = run_common::resolve_lang(args.lang.as_deref(), args.source_path.as_deref());

    let query = match QueryBuilder::new(source_map).parse() {
        Ok(parsed) => parsed.analyze().link(&lang),
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    };

    if !query.is_valid() {
        eprint!(
            "{}",
            query
                .diagnostics()
                .render_colored(query.source_map(), args.color)
        );
        std::process::exit(1);
    }

    let bytecode = emit_linked(&query).expect("emit failed");
    let module = Module::from_bytes(bytecode).expect("module load failed");

    let entrypoint = run_common::resolve_entrypoint(&module, args.entry.as_deref());
    let tree = lang.parse(&source_code);
    let trivia_types = run_common::build_trivia_types(&module);

    let vm = VM::new(&tree, trivia_types, FuelLimits::default());
    let effects = match vm.execute(&module, &entrypoint) {
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
    let value = materializer.materialize(effects.as_slice(), entrypoint.result_type);

    let colors = Colors::new(args.color);

    // Debug-only: verify output matches declared type
    debug_verify_type(&value, entrypoint.result_type, &module, colors);

    let output = value.format(args.pretty, colors);
    println!("{}", output);
}
