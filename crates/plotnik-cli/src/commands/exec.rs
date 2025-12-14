use std::fs;
use std::io::{self, Read};
use std::path::PathBuf;

use plotnik_langs::{Lang, NodeFieldId, NodeTypeId};
use plotnik_lib::Query;
use plotnik_lib::engine::interpreter::QueryInterpreter;
use plotnik_lib::engine::validate::validate as validate_result;
use plotnik_lib::engine::value::{ResolvedValue, VerboseResolvedValue};
use plotnik_lib::ir::{NodeKindResolver, QueryEmitter};

use super::debug::source::resolve_lang;

pub struct ExecArgs {
    pub query_text: Option<String>,
    pub query_file: Option<PathBuf>,
    pub source_text: Option<String>,
    pub source_file: Option<PathBuf>,
    pub lang: Option<String>,
    pub pretty: bool,
    pub verbose_nodes: bool,
    pub check: bool,
}

struct LangResolver(Lang);

impl NodeKindResolver for LangResolver {
    fn resolve_kind(&self, name: &str) -> Option<NodeTypeId> {
        self.0.resolve_named_node(name)
    }

    fn resolve_field(&self, name: &str) -> Option<NodeFieldId> {
        self.0.resolve_field(name)
    }
}

pub fn run(args: ExecArgs) {
    if let Err(msg) = validate(&args) {
        eprintln!("error: {}", msg);
        std::process::exit(1);
    }

    let query_source = load_query(&args);
    if query_source.trim().is_empty() {
        eprintln!("error: query cannot be empty");
        std::process::exit(1);
    }
    let source_code = load_source(&args);
    let lang = resolve_lang(&args.lang, &args.source_text, &args.source_file);

    // Parse and validate query
    let mut query = Query::new(&query_source).exec().unwrap_or_else(|e| {
        eprintln!("error: {}", e);
        std::process::exit(1);
    });

    if !query.is_valid() {
        eprint!("{}", query.diagnostics().render(&query_source));
        std::process::exit(1);
    }

    // Link query against language
    query.link(&lang);
    if !query.is_valid() {
        eprint!("{}", query.diagnostics().render(&query_source));
        std::process::exit(1);
    }

    // Build transition graph and type info
    let mut query = query.build_graph();
    if query.has_type_errors() {
        eprint!("{}", query.diagnostics().render(&query_source));
        std::process::exit(1);
    }

    // Auto-wrap definitions with root node if available
    if let Some(root_id) = lang.root() {
        if let Some(root_kind) = lang.node_type_name(root_id) {
            query = query.wrap_with_root(root_kind);
        }
    }

    // Emit compiled query
    let resolver = LangResolver(lang.clone());
    let emitter = QueryEmitter::new(query.graph(), query.type_info(), resolver);
    let compiled = emitter.emit().unwrap_or_else(|e| {
        eprintln!("error: emit failed: {:?}", e);
        std::process::exit(1);
    });

    // Parse source
    let tree = lang.parse(&source_code);
    let cursor = tree.walk();

    // Run interpreter
    let interpreter = QueryInterpreter::new(&compiled, cursor, &source_code);
    let result = interpreter.run().unwrap_or_else(|e| {
        eprintln!("error: {}", e);
        std::process::exit(1);
    });

    // Type checking against inferred types
    if args.check {
        let expected_type = compiled.entrypoints().first().map(|e| e.result_type());
        if let Some(type_id) = expected_type
            && let Err(e) = validate_result(&result, type_id, &compiled)
        {
            eprintln!("type error: {}", e);
            std::process::exit(1);
        }
    }

    // Output JSON
    let output = match (args.verbose_nodes, args.pretty) {
        (true, true) => serde_json::to_string_pretty(&VerboseResolvedValue(&result, &compiled)),
        (true, false) => serde_json::to_string(&VerboseResolvedValue(&result, &compiled)),
        (false, true) => serde_json::to_string_pretty(&ResolvedValue(&result, &compiled)),
        (false, false) => serde_json::to_string(&ResolvedValue(&result, &compiled)),
    };

    match output {
        Ok(json) => println!("{}", json),
        Err(e) => {
            eprintln!("error: JSON serialization failed: {}", e);
            std::process::exit(1);
        }
    }
}

fn load_query(args: &ExecArgs) -> String {
    if let Some(ref text) = args.query_text {
        return text.clone();
    }
    if let Some(ref path) = args.query_file {
        if path.as_os_str() == "-" {
            let mut buf = String::new();
            io::stdin()
                .read_to_string(&mut buf)
                .expect("failed to read stdin");
            return buf;
        }
        return fs::read_to_string(path).expect("failed to read query file");
    }
    unreachable!("validation ensures query input exists")
}

fn load_source(args: &ExecArgs) -> String {
    if let Some(ref text) = args.source_text {
        return text.clone();
    }
    if let Some(ref path) = args.source_file {
        if path.as_os_str() == "-" {
            panic!("cannot read both query and source from stdin");
        }
        return fs::read_to_string(path).expect("failed to read source file");
    }
    unreachable!("validation ensures source input exists")
}

fn validate(args: &ExecArgs) -> Result<(), &'static str> {
    let has_query = args.query_text.is_some() || args.query_file.is_some();
    let has_source = args.source_text.is_some() || args.source_file.is_some();

    if !has_query {
        return Err("query is required: use -q/--query or --query-file");
    }

    if !has_source {
        return Err("source is required: use -s/--source-file or --source");
    }

    if args.source_text.is_some() && args.lang.is_none() {
        return Err("--lang is required when using --source");
    }

    Ok(())
}
