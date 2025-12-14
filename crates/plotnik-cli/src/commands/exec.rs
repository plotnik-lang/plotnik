use std::fs;
use std::io::{self, Read};
use std::path::PathBuf;

use plotnik_langs::{Lang, NodeFieldId, NodeTypeId};
use plotnik_lib::Query;
use plotnik_lib::engine::interpreter::QueryInterpreter;
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
    let query = query.build_graph();
    if query.has_type_errors() {
        eprint!("{}", query.diagnostics().render(&query_source));
        std::process::exit(1);
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
        todo!("validate result against compiled.type_metadata()")
    }

    // Output JSON
    let output = if args.verbose_nodes {
        todo!("serialize with VerboseNode instead of CapturedNode")
    } else if args.pretty {
        serde_json::to_string_pretty(&result)
    } else {
        serde_json::to_string(&result)
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
