use std::io::Read as _;
use std::path::{Path, PathBuf};

use clap::ArgMatches;

use crate::error::CliError;

pub struct Inputs<'a> {
    pub query_path: Option<&'a Path>,
    pub query_text: Option<&'a str>,
    pub source_path: Option<&'a Path>,
    pub source_text: Option<&'a str>,
}

impl<'a> Inputs<'a> {
    pub fn from_matches(m: &'a ArgMatches) -> Result<Self, CliError> {
        let mut query_path = m.get_one::<PathBuf>("query_path").map(PathBuf::as_path);
        let query_text = m.get_one::<String>("query_text").map(String::as_str);
        let mut source_path = m.get_one::<PathBuf>("source_path").map(PathBuf::as_path);
        let source_text = m.get_one::<String>("source_text").map(String::as_str);
        // With an inline query, clap puts a lone SOURCE in the first optional
        // positional slot. Interpret it before checking for conflicting inputs.
        if query_text.is_some() && source_path.is_none() && source_text.is_none() {
            source_path = query_path.take();
        }
        if query_text.is_some() && query_path.is_some() {
            return Err(CliError::fatal("use either a query path or --query"));
        }
        if source_text.is_some() && source_path.is_some() {
            return Err(CliError::fatal("use either a source path or --source"));
        }
        if query_path == Some(Path::new("-")) && source_path == Some(Path::new("-")) {
            return Err(CliError::fatal("query and source cannot both read stdin"));
        }
        Ok(Self {
            query_path,
            query_text,
            source_path,
            source_text,
        })
    }

    pub fn source(&self) -> Result<String, CliError> {
        if let Some(source) = self.source_text {
            return Ok(source.to_owned());
        }
        let path = self
            .source_path
            .ok_or_else(|| CliError::fatal("source required: use SOURCE or --source"))?;
        if path == Path::new("-") {
            let mut source = String::new();
            std::io::stdin()
                .read_to_string(&mut source)
                .map_err(|error| {
                    CliError::fatal(format!("failed to read source stdin: {error}"))
                })?;
            return Ok(source);
        }
        std::fs::read_to_string(path).map_err(|error| {
            CliError::fatal(format!(
                "failed to read source '{}': {error}",
                path.display()
            ))
        })
    }
}
