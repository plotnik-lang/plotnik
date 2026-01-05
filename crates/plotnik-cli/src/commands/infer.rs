use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

use plotnik_lib::Colors;
use plotnik_lib::QueryBuilder;
use plotnik_lib::bytecode::Module;
use plotnik_lib::typegen::typescript;

use super::lang_resolver::{resolve_lang, resolve_lang_required, suggest_language};
use super::query_loader::load_query_source;

pub struct InferArgs {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub lang: Option<String>,
    pub format: String,
    pub verbose_nodes: bool,
    pub no_node_type: bool,
    pub export: bool,
    pub output: Option<PathBuf>,
    pub color: bool,
    pub void_type: Option<String>,
}

pub fn run(args: InferArgs) {
    // Validate format
    let fmt = args.format.to_lowercase();
    if fmt != "typescript" && fmt != "ts" {
        eprintln!("error: --format must be 'typescript' or 'ts'");
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

    // Parse and analyze
    let query = match QueryBuilder::new(source_map).parse() {
        Ok(parsed) => parsed.analyze(),
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    };

    // Resolve language (optional - enables linking)
    let lang = if let Some(lang_name) = &args.lang {
        match resolve_lang_required(lang_name) {
            Ok(l) => Some(l),
            Err(msg) => {
                eprintln!("error: {}", msg);
                if let Some(suggestion) = suggest_language(lang_name) {
                    eprintln!();
                    eprintln!("Did you mean '{}'?", suggestion);
                }
                eprintln!();
                eprintln!("Run 'plotnik langs' for the full list.");
                std::process::exit(1);
            }
        }
    } else {
        resolve_lang(None, args.query_path.as_deref())
    };

    let bytecode = if let Some(lang) = lang {
        let linked = query.link(&lang);
        if !linked.is_valid() {
            eprint!(
                "{}",
                linked
                    .diagnostics()
                    .render_colored(linked.source_map(), args.color)
            );
            std::process::exit(1);
        }
        linked.emit().expect("bytecode emission failed")
    } else {
        if !query.is_valid() {
            eprint!(
                "{}",
                query
                    .diagnostics()
                    .render_colored(query.source_map(), args.color)
            );
            std::process::exit(1);
        }
        query.emit().expect("bytecode emission failed")
    };
    let module = Module::from_bytes(bytecode).expect("module loading failed");

    // Emit TypeScript types
    let void_type = match args.void_type.as_deref() {
        Some("null") => typescript::VoidType::Null,
        _ => typescript::VoidType::Undefined,
    };
    // Only use colors when outputting to stdout (not to file)
    let use_colors = args.color && args.output.is_none();
    let config = typescript::Config {
        export: args.export,
        emit_node_type: !args.no_node_type,
        verbose_nodes: args.verbose_nodes,
        void_type,
        colors: Colors::new(use_colors),
    };
    let output = typescript::emit_with_config(&module, config);

    // Write output
    if let Some(ref path) = args.output {
        fs::write(path, &output).unwrap_or_else(|e| {
            eprintln!("error: failed to write '{}': {}", path.display(), e);
            std::process::exit(1);
        });
        // Success message
        let type_count = count_types(&output);
        eprintln!("Wrote {} types to {}", type_count, path.display());
    } else {
        io::stdout().write_all(output.as_bytes()).unwrap();
    }
}

fn count_types(output: &str) -> usize {
    output
        .lines()
        .filter(|line| {
            line.starts_with("export type ")
                || line.starts_with("type ")
                || line.starts_with("export interface ")
                || line.starts_with("interface ")
        })
        .count()
}
