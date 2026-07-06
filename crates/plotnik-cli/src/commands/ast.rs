//! Show AST of query and/or source file.

use std::path::PathBuf;

use plotnik_lib::{QueryBuilder, dump_tree_text, tree_to_json};
use serde_json::json;

use super::lang_resolver::reconcile_lang;
use super::query_loader::load_query;
use super::run_common;
use crate::error::{CliError, CliResult};

pub struct AstArgs {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub source_path: Option<PathBuf>,
    pub source_text: Option<String>,
    pub lang: Option<String>,
    pub raw: bool,
    pub json: bool,
    pub color: bool,
}

pub fn run(args: AstArgs) -> CliResult {
    let has_query = args.query_path.is_some() || args.query_text.is_some();
    let has_source = args.source_path.is_some() || args.source_text.is_some();

    if !has_query && !has_source {
        return Err(CliError::fatal("query or source required"));
    }
    if args.json {
        if !has_source {
            return Err(CliError::fatal("--json requires source input"));
        }
        let declared_lang = if has_query {
            query_declared_lang(&args)?
        } else {
            None
        };
        print_source_ast(&args, declared_lang.as_deref())?;
        return Ok(());
    }

    let show_headers = has_query && has_source;

    let mut declared_lang = None;
    if has_query {
        if show_headers {
            println!("# Query AST");
        }
        declared_lang = print_query_ast(&args)?;
    }

    if has_source {
        if show_headers {
            println!("\n# Source AST");
        }
        print_source_ast(&args, declared_lang.as_deref())?;
    }

    Ok(())
}

/// Reads query metadata needed by source AST rendering; no query AST is emitted.
fn query_declared_lang(args: &AstArgs) -> Result<Option<String>, CliError> {
    let loaded = load_query(args.query_path.as_deref(), args.query_text.as_deref())?;

    if loaded.sources.is_empty() {
        return Err(CliError::fatal("query cannot be empty"));
    }

    reconcile_lang(args.lang.as_deref(), loaded.shebang.lang.as_deref())?;
    Ok(loaded.shebang.lang)
}

/// Prints the query AST; returns the shebang-declared language, if any.
fn print_query_ast(args: &AstArgs) -> Result<Option<String>, CliError> {
    let loaded = load_query(args.query_path.as_deref(), args.query_text.as_deref())?;

    if loaded.sources.is_empty() {
        return Err(CliError::fatal("query cannot be empty"));
    }

    // Enforce -l/shebang agreement even when no source AST follows
    reconcile_lang(args.lang.as_deref(), loaded.shebang.lang.as_deref())?;

    let query = QueryBuilder::new(loaded.sources)
        .analyze()
        .map_err(|e| CliError::fatal(e.to_string()))?;

    if query.diagnostics().has_errors() || query.diagnostics().has_warnings() {
        eprint!(
            "{}",
            query
                .diagnostics()
                .render_colored(query.source_map(), args.color)
        );
    }

    let output = if args.raw {
        query.dump_cst_with_trivia(true)
    } else {
        query.dump_ast()
    };
    print!("{}", output);

    Ok(loaded.shebang.lang)
}

fn print_source_ast(args: &AstArgs, declared_lang: Option<&str>) -> CliResult {
    let source = run_common::load_source(
        args.source_text.as_deref(),
        args.source_path.as_deref(),
        args.query_path.as_deref(),
    )?;
    let lang = run_common::resolve_run_lang(
        args.lang.as_deref(),
        declared_lang,
        args.source_path.as_deref(),
    )?;
    let tree = lang.parse_source(&source);
    if args.json {
        let output = json!({
            "source": tree_to_json(&tree, &source, args.raw),
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&output).expect("source tree JSON serializes")
        );
        return Ok(());
    }

    print!(
        "{}",
        dump_tree_text(&tree, &source, lang.grammar(), args.raw)
    );

    Ok(())
}
