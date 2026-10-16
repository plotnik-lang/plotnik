use std::fs;
use std::io::{self, Read};
use std::path::Path;

use plotnik_compiler::{SourceCollection, SourceMap, SourcePath};

use crate::error::CliError;

pub struct QuerySources {
    pub sources: SourceMap,
    pub is_directory: bool,
}

impl QuerySources {
    pub fn into_collection(self) -> SourceCollection {
        let ids: Vec<_> = self.sources.iter().map(|source| source.id).collect();
        let mut collection = SourceCollection::new(self.sources);
        for id in ids {
            if self.is_directory {
                collection.include_directory(id);
            } else {
                collection.include(id);
            }
        }

        collection
    }
}

pub fn load_query(path: &Path) -> Result<QuerySources, CliError> {
    if path.as_os_str() == "-" {
        return load_stdin();
    }

    if path.is_dir() {
        return load_workspace(path);
    }

    load_file(path)
}

fn load_stdin() -> Result<QuerySources, CliError> {
    let mut buf = String::new();
    io::stdin()
        .read_to_string(&mut buf)
        .map_err(|error| CliError::fatal(format!("failed to read stdin: {error}")))?;
    let mut sources = SourceMap::new();
    sources.add_stdin(&buf);
    Ok(QuerySources {
        sources,
        is_directory: false,
    })
}

fn load_file(path: &Path) -> Result<QuerySources, CliError> {
    let content = read_file(path)?;
    let mut sources = SourceMap::new();
    let source_path = path.to_string_lossy();
    sources.add_file(SourcePath::new(&source_path), &content);
    Ok(QuerySources {
        sources,
        is_directory: false,
    })
}

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

    paths.sort();

    let mut sources = SourceMap::new();
    for path in paths {
        let content = read_file(&path)?;
        let source_path = path.to_string_lossy();
        sources.add_file(SourcePath::new(&source_path), &content);
    }

    Ok(QuerySources {
        sources,
        is_directory: true,
    })
}

fn read_file(path: &Path) -> Result<String, CliError> {
    fs::read_to_string(path)
        .map_err(|error| CliError::fatal(format!("failed to read '{}': {error}", path.display())))
}
