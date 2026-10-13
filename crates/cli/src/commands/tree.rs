use std::path::PathBuf;

use clap::ArgMatches;
use plotnik_compiler::{QueryBuilder, dump_tree_text};

use super::{inputs::load_source, language::LanguageOptions, query_loader::load_query};
use crate::cli::ColorChoice;
use crate::error::{CliError, CliResult, write_stderr, write_stdout};

pub fn run(m: &ArgMatches) -> CliResult {
    let path = m.get_one::<PathBuf>("input_path").map(PathBuf::as_path);
    let query_text = m.get_one::<String>("query_text").map(String::as_str);
    let source_text = m.get_one::<String>("source_text").map(String::as_str);
    let color = ColorChoice::from_matches(m).should_colorize();
    let language = LanguageOptions::from_matches(m);
    if path.is_some_and(|path| path.extension().is_none()) && language.lang.is_none() {
        return Err(CliError::fatal(
            "input language required: use --lang (plotnik for query syntax)",
        ));
    }

    let is_query = query_text.is_some()
        || path.is_some_and(|path| match path.extension() {
            Some(extension) => extension == "ptk",
            None => language.lang.as_deref() == Some("plotnik"),
        });
    if is_query {
        let loaded = load_query(path, query_text)?;
        let query = QueryBuilder::new(loaded.sources)
            .analyze()
            .map_err(|error| CliError::fatal(error.to_string()))?;
        if !query.diagnostics().is_empty() {
            write_stderr(format_args!(
                "{}",
                query
                    .diagnostics()
                    .render_colored(query.source_map(), color)
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
        write_stdout(format_args!("{rendered}"))?;
        if query.diagnostics().has_errors() {
            return Err(CliError::No);
        }

        return Ok(());
    }

    let source = load_source(path, source_text)?;
    let mut parser = language.load(path)?.parser()?;
    let tree = parser
        .parse(&source)
        .map_err(|error| CliError::fatal(error.to_string()))?;
    let anonymous = m.get_flag("include_anonymous");
    write_stdout(format_args!(
        "{}",
        dump_tree_text(&tree, &source, parser.grammar(), anonymous)
    ))
}
