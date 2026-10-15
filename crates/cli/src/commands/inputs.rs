use std::io::Read as _;
use std::path::{Path, PathBuf};

use clap::ArgMatches;

use crate::error::CliError;

pub struct Inputs<'a> {
    pub query_path: Option<&'a Path>,
    /// No named entry means a leading '-' selected the stdin expression.
    pub entry: Option<&'a str>,
    pub source_path: &'a Path,
}

impl<'a> Inputs<'a> {
    pub fn from_matches(m: &'a ArgMatches) -> Result<Self, CliError> {
        let paths: Vec<_> = m
            .get_many::<PathBuf>("inputs")
            .expect("clap requires positional input")
            .map(PathBuf::as_path)
            .collect();
        let (source_path, query_paths) = paths.split_last().expect("clap requires inputs");
        let expression = query_paths
            .first()
            .is_some_and(|path| *path == Path::new("-"));
        let query_path = match (expression, query_paths) {
            (true, [_]) => None,
            (true, [_, path]) | (false, [path]) => Some(*path),
            _ => {
                return Err(CliError::fatal(
                    "expected one saved PTK path, or leading '-' with an optional saved PTK path, then one source FILE",
                ));
            }
        };
        if *source_path == Path::new("-") || query_path == Some(Path::new("-")) {
            return Err(CliError::fatal(
                "only a leading '-' can read stdin; the source must be a file",
            ));
        }

        let entry = m.get_one::<String>("entry").map(String::as_str);
        if expression && entry.is_some() {
            return Err(CliError::fatal(
                "--entry conflicts with the stdin expression entrypoint",
            ));
        }

        if !expression && entry.is_none() {
            return Err(CliError::fatal("saved PTK input requires --entry NAME"));
        }

        Ok(Self {
            query_path,
            entry,
            source_path,
        })
    }
}

pub fn load_source(path: Option<&Path>, text: Option<&str>) -> Result<String, CliError> {
    if let Some(text) = text {
        return Ok(text.to_owned());
    }

    let path = path.ok_or_else(|| {
        CliError::fatal("source required: supply a file path or --source for inline text")
    })?;
    if path == Path::new("-") {
        let mut source = String::new();
        std::io::stdin()
            .read_to_string(&mut source)
            .map_err(|error| CliError::fatal(format!("failed to read source stdin: {error}")))?;
        return Ok(source);
    }

    std::fs::read_to_string(path).map_err(|error| {
        CliError::fatal(format!(
            "failed to read source '{}': {error}",
            path.display()
        ))
    })
}
