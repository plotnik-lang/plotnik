//! Show the query tree and/or source syntax tree.

use std::path::PathBuf;

use plotnik_lib::{QueryBuilder, dump_tree_text, tree_to_json};
use serde_json::{Map, Value, json};

use super::lang_resolver::reconcile_lang;
use super::query_loader::load_query;
use super::run_common;
use crate::error::{CliError, CliResult, write_stderr, write_stdout, writeln_stdout};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryView {
    Ast,
    Cst,
}

pub struct TreeArgs {
    pub query_path: Option<PathBuf>,
    pub query_text: Option<String>,
    pub source_path: Option<PathBuf>,
    pub source_text: Option<String>,
    pub lang: Option<String>,
    pub query_view: QueryView,
    pub include_anonymous: bool,
    pub json: bool,
    pub color: bool,
}

pub fn run(args: TreeArgs) -> CliResult {
    run_common::reject_ambiguous_inputs(
        args.query_text.as_deref(),
        args.query_path.as_deref(),
        args.source_text.as_deref(),
        args.source_path.as_deref(),
    )?;

    let has_query = args.query_path.is_some() || args.query_text.is_some();
    let has_source = args.source_path.is_some() || args.source_text.is_some();

    if !has_query && !has_source {
        return Err(CliError::fatal("query or source required"));
    }
    if args.json {
        print_json(&args, has_query, has_source)?;
        return Ok(());
    }

    let show_headers = has_query && has_source;

    let mut declared_lang = None;
    if has_query {
        if show_headers {
            writeln_stdout(format_args!("# {}", query_label(args.query_view)))?;
        }
        declared_lang = print_query_tree(&args)?;
    }

    if has_source {
        if show_headers {
            writeln_stdout(format_args!("\n# Source syntax tree"))?;
        }
        print_source_tree(&args, declared_lang.as_deref())?;
    }

    Ok(())
}

fn query_label(view: QueryView) -> &'static str {
    match view {
        QueryView::Ast => "Query AST",
        QueryView::Cst => "Query CST",
    }
}

/// Prints the selected query tree; returns the shebang-declared language, if any.
fn print_query_tree(args: &TreeArgs) -> Result<Option<String>, CliError> {
    let (output, declared_lang) = render_query_tree(args)?;
    write_stdout(format_args!("{output}"))?;
    Ok(declared_lang)
}

fn render_query_tree(args: &TreeArgs) -> Result<(String, Option<String>), CliError> {
    let loaded = load_query(args.query_path.as_deref(), args.query_text.as_deref())?;

    if loaded.sources.is_empty() {
        return Err(CliError::fatal("query cannot be empty"));
    }

    // Enforce -l/shebang agreement even when no source tree follows.
    reconcile_lang(args.lang.as_deref(), loaded.shebang.lang.as_deref())?;

    let query = QueryBuilder::new(loaded.sources)
        .analyze()
        .map_err(|e| CliError::fatal(e.to_string()))?;

    if query.diagnostics().has_errors() || query.diagnostics().has_warnings() {
        write_stderr(format_args!(
            "{}",
            query
                .diagnostics()
                .render_colored(query.source_map(), args.color)
        ))?;
    }

    let output = match args.query_view {
        QueryView::Ast => query.dump_ast(),
        QueryView::Cst => query.dump_cst_with_trivia(true),
    };
    Ok((output, loaded.shebang.lang))
}

fn source_tree_json(args: &TreeArgs, declared_lang: Option<&str>) -> Result<Value, CliError> {
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
    Ok(tree_to_json(&tree, &source, args.include_anonymous))
}

fn print_source_tree(args: &TreeArgs, declared_lang: Option<&str>) -> CliResult {
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

    write_stdout(format_args!(
        "{}",
        dump_tree_text(&tree, &source, lang.grammar(), args.include_anonymous)
    ))?;

    Ok(())
}

fn print_json(args: &TreeArgs, has_query: bool, has_source: bool) -> CliResult {
    let mut output = Map::new();
    let mut declared_lang = None;

    if has_query {
        let (query_tree, lang) = render_query_tree(args)?;
        output.insert("query_tree".to_string(), json!(query_tree));
        declared_lang = lang;
    }
    if has_source {
        output.insert(
            "source_tree".to_string(),
            source_tree_json(args, declared_lang.as_deref())?,
        );
    }

    writeln_stdout(format_args!(
        "{}",
        serde_json::to_string_pretty(&Value::Object(output)).expect("tree JSON serializes")
    ))?;
    Ok(())
}
