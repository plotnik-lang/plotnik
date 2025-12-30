use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;

use plotnik_langs::Lang;
use plotnik_lib::Query;
use plotnik_lib::bytecode::emit::{TsEmitConfig, emit_typescript_with_config};

pub struct TypesArgs {
    pub query_text: Option<String>,
    pub query_file: Option<PathBuf>,
    pub lang: Option<String>,
    pub format: String,
    pub verbose_nodes: bool,
    pub no_node_type: bool,
    pub export: bool,
    pub output: Option<PathBuf>,
}

pub fn run(args: TypesArgs) {
    if let Err(msg) = validate(&args) {
        eprintln!("error: {}", msg);
        std::process::exit(1);
    }

    let query_source = load_query(&args);
    if query_source.trim().is_empty() {
        eprintln!("error: query cannot be empty");
        std::process::exit(1);
    }
    let lang = resolve_lang_required(&args.lang);

    // Parse and analyze query
    let query = Query::try_from(query_source.as_str())
        .unwrap_or_else(|e| {
            eprintln!("error: {}", e);
            std::process::exit(1);
        })
        .link(&lang);

    if !query.is_valid() {
        eprint!("{}", query.diagnostics().render(query.source_map()));
        std::process::exit(1);
    }

    // Emit to bytecode first
    let bytecode = query.emit().expect("bytecode emission failed");
    let module =
        plotnik_lib::bytecode::Module::from_bytes(bytecode).expect("module loading failed");

    // Emit TypeScript types from bytecode
    let config = TsEmitConfig {
        export: args.export,
        emit_node_type: !args.no_node_type,
        verbose_nodes: args.verbose_nodes,
    };

    let output = emit_typescript_with_config(&module, config);

    // Write output
    if let Some(ref path) = args.output {
        fs::write(path, &output).unwrap_or_else(|e| {
            eprintln!("error: failed to write {}: {}", path.display(), e);
            std::process::exit(1);
        });
    } else {
        io::stdout().write_all(output.as_bytes()).unwrap();
    }
}

fn load_query(args: &TypesArgs) -> String {
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
        return fs::read_to_string(path).unwrap_or_else(|e| {
            eprintln!("error: failed to read query file: {}", e);
            std::process::exit(1);
        });
    }
    unreachable!("validation ensures query input exists")
}

fn resolve_lang_required(lang: &Option<String>) -> Lang {
    let name = lang.as_ref().expect("--lang is required");
    plotnik_langs::from_name(name).unwrap_or_else(|| {
        eprintln!("error: unknown language: {}", name);
        std::process::exit(1);
    })
}

fn validate(args: &TypesArgs) -> Result<(), &'static str> {
    let has_query = args.query_text.is_some() || args.query_file.is_some();

    if !has_query {
        return Err("query is required: use -q/--query or --query-file");
    }

    if args.lang.is_none() {
        return Err("--lang is required for type generation");
    }

    let fmt = args.format.to_lowercase();
    if fmt != "typescript" && fmt != "ts" {
        return Err("--format must be 'typescript' or 'ts'");
    }

    Ok(())
}
