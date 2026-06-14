use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

use plotnik_lib::QueryBuilder;
use plotnik_lib::bytecode::Module;
use plotnik_lib::typegen::typescript;

use super::lang_resolver::require_lang;
use super::query_loader::load_query_source;
use crate::error::{CliError, CliResult};

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

pub fn run(args: InferArgs) -> CliResult {
    // Validate format
    let fmt = args.format.to_lowercase();
    if fmt != "typescript" && fmt != "ts" {
        return Err(CliError::fatal("--format must be 'typescript' or 'ts'"));
    }

    let loaded = load_query_source(args.query_path.as_deref(), args.query_text.as_deref())?;

    if loaded.sources.is_empty() {
        return Err(CliError::fatal("query cannot be empty"));
    }

    // Parse and analyze
    let query = QueryBuilder::new(loaded.sources)
        .parse()
        .map_err(|e| CliError::fatal(e.to_string()))?
        .analyze();

    let lang = require_lang(
        args.lang.as_deref(),
        loaded.shebang.lang.as_deref(),
        args.query_path.as_deref(),
        "infer",
    )?;

    let linked = query.link(lang.grammar());
    if !linked.is_valid() {
        eprint!(
            "{}",
            linked
                .diagnostics()
                .render_colored(linked.source_map(), args.color)
        );
        return Err(CliError::FatalRendered);
    }
    let bytecode = linked.emit().map_err(|e| CliError::fatal(e.to_string()))?;
    let module = Module::load(&bytecode).expect("module loading failed");

    // Emit TypeScript types
    let void_type = match args.void_type.as_deref() {
        Some("null") => typescript::VoidType::Null,
        _ => typescript::VoidType::Undefined,
    };
    // Only use colors when outputting to stdout (not to file)
    let use_colors = args.color && args.output.is_none();
    let config = typescript::Config::new()
        .export(args.export)
        .emit_node_type(!args.no_node_type)
        .verbose_nodes(args.verbose_nodes)
        .void_type(void_type)
        .colored(use_colors);
    let output = typescript::emit_with_config(&module, config);

    // Write output
    if let Some(ref path) = args.output {
        fs::write(path, &output)
            .map_err(|e| CliError::fatal(format!("failed to write '{}': {}", path.display(), e)))?;
        // Success message
        let type_count = count_types(&output);
        eprintln!("Wrote {} types to {}", type_count, path.display());
    } else {
        io::stdout().write_all(output.as_bytes()).unwrap();
    }

    Ok(())
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
