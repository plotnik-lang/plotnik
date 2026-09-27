use std::fs;
use std::io::{self, Read};
use std::path::Path;

use plotnik_lib::{SourceMap, SourcePath};

use crate::error::CliError;

pub struct QuerySources {
    pub sources: SourceMap,
}

pub fn load_query(
    query_path: Option<&Path>,
    query_text: Option<&str>,
) -> Result<QuerySources, CliError> {
    if let Some(text) = query_text {
        let mut sources = SourceMap::new();
        sources.add_inline(text);
        return Ok(QuerySources { sources });
    }

    if let Some(path) = query_path {
        if path.as_os_str() == "-" {
            return load_stdin();
        }
        if path.is_dir() {
            return load_workspace(path);
        }
        return load_file(path);
    }

    Err(CliError::fatal(
        "query is required: use positional argument or -q/--query",
    ))
}

fn load_stdin() -> Result<QuerySources, CliError> {
    let mut buf = String::new();
    io::stdin()
        .read_to_string(&mut buf)
        .map_err(|error| CliError::fatal(format!("failed to read stdin: {error}")))?;
    let mut sources = SourceMap::new();
    sources.add_stdin(&buf);
    Ok(QuerySources { sources })
}

fn load_file(path: &Path) -> Result<QuerySources, CliError> {
    let content = read_file(path)?;
    let mut sources = SourceMap::new();
    let source_path = path.to_string_lossy();
    sources.add_file(SourcePath::new(&source_path), &content);
    Ok(QuerySources { sources })
}

/// Merge all `.ptk` files in a directory into one namespace.
fn load_workspace(dir: &Path) -> Result<QuerySources, CliError> {
    let entries = fs::read_dir(dir).map_err(|error| {
        CliError::fatal(format!(
            "failed to read directory '{}': {error}",
            dir.display()
        ))
    })?;
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            CliError::fatal(format!(
                "failed to read an entry of directory '{}': {error}",
                dir.display()
            ))
        })?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "ptk") {
            paths.push(path);
        }
    }

    if paths.is_empty() {
        return Err(CliError::fatal(format!(
            "no .ptk files found in workspace '{}'",
            dir.display()
        )));
    }

    paths.sort();

    let mut sources = SourceMap::new();
    for path in paths {
        let content = read_file(&path)?;
        let source_path = path.to_string_lossy();
        sources.add_file(SourcePath::new(&source_path), &content);
    }

    Ok(QuerySources { sources })
}

fn read_file(path: &Path) -> Result<String, CliError> {
    fs::read_to_string(path)
        .map_err(|error| CliError::fatal(format!("failed to read '{}': {error}", path.display())))
}
