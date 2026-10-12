use clap::ArgMatches;
use plotnik_lib::{QueryBuilder, dump_tree_text, tree_to_json};
use serde_json::json;

use super::{inputs::Inputs, language, query_loader::load_query};
use crate::cli::ColorChoice;
use crate::error::{CliError, CliResult, write_stderr, write_stdout, writeln_stdout};

pub fn run(m: &ArgMatches) -> CliResult {
    let mut inputs = Inputs::from_matches(m)?;
    if inputs.query_text.is_none()
        && inputs.source_path.is_none()
        && inputs.source_text.is_none()
        && inputs
            .query_path
            .is_some_and(|path| path.extension().is_none_or(|ext| ext != "ptk"))
    {
        inputs.source_path = inputs.query_path.take();
    }
    let has_query = inputs.query_path.is_some() || inputs.query_text.is_some();
    let has_source = inputs.source_path.is_some() || inputs.source_text.is_some();
    if !has_query && !has_source {
        return Err(CliError::fatal("query or source required"));
    }
    let mut output = json!({});
    if has_query {
        let loaded = load_query(inputs.query_path, inputs.query_text)?;
        let query = QueryBuilder::new(loaded.sources)
            .analyze()
            .map_err(|error| CliError::fatal(error.to_string()))?;
        if !query.diagnostics().is_empty() {
            write_stderr(format_args!(
                "{}",
                query.diagnostics().render_colored(
                    query.source_map(),
                    ColorChoice::from_matches(m).should_colorize()
                )
            ))?;
        }
        let view = m
            .get_one::<String>("query_view")
            .expect("clap supplies query view");
        let rendered = if view == "ast" {
            query.dump_ast()
        } else {
            query.dump_cst_with_trivia(true)
        };
        if m.get_flag("json") {
            output["query_tree"] = json!(rendered);
        } else {
            write_stdout(format_args!("{rendered}"))?;
        }
        if query.diagnostics().has_errors() {
            return Err(CliError::No);
        }
    }
    if has_source {
        let source = inputs.source()?;
        let mut parser = language::load(m, inputs.source_path)?.parser();
        let tree = parser
            .parse(&source)
            .map_err(|error| CliError::fatal(error.to_string()))?;
        let anonymous = m.get_flag("include_anonymous");
        if m.get_flag("json") {
            output["source_tree"] = tree_to_json(&tree, &source, anonymous);
        } else {
            write_stdout(format_args!(
                "{}",
                dump_tree_text(&tree, &source, parser.grammar(), anonymous)
            ))?;
        }
    }
    if m.get_flag("json") {
        writeln_stdout(format_args!("{output}"))?;
    }
    Ok(())
}
