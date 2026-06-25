use std::fs;
use std::io::{self, Read};
use std::path::Path;

use plotnik_lib::{SourceMap, SourcePath};

use crate::cli::shebang::{ShebangDecl, parse_shebang};
use crate::error::CliError;

/// Query sources plus the semantic options declared in their shebang lines.
pub struct QuerySources {
    pub sources: SourceMap,
    pub shebang: ShebangDecl,
}

pub fn load_query(
    query_path: Option<&Path>,
    query_text: Option<&str>,
) -> Result<QuerySources, CliError> {
    if let Some(text) = query_text {
        // Inline text can carry a shebang too (e.g. `-q "$(cat q.ptk)"`)
        let shebang = extract_shebang(text, "<query>")?;
        let mut sources = SourceMap::new();
        sources.add_inline(text);
        return Ok(QuerySources { sources, shebang });
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
        .map_err(|e| CliError::fatal(format!("failed to read stdin: {}", e)))?;
    let shebang = extract_shebang(&buf, "<stdin>")?;
    let mut sources = SourceMap::new();
    sources.add_stdin(&buf);
    Ok(QuerySources { sources, shebang })
}

fn load_file(path: &Path) -> Result<QuerySources, CliError> {
    let content = read_file(path)?;
    let shebang = extract_shebang(&content, &path.display().to_string())?;
    let mut sources = SourceMap::new();
    let source_path = path.to_string_lossy();
    sources.add_file(SourcePath::new(&source_path), &content);
    Ok(QuerySources { sources, shebang })
}

/// Terraform-style workspace: all `.ptk` files in the directory are merged
/// into one namespace. Shebang declarations across files must agree.
fn load_workspace(dir: &Path) -> Result<QuerySources, CliError> {
    let entries = fs::read_dir(dir).map_err(|e| {
        CliError::fatal(format!(
            "failed to read directory '{}': {}",
            dir.display(),
            e
        ))
    })?;
    let mut paths: Vec<_> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "ptk"))
        .collect();

    if paths.is_empty() {
        return Err(CliError::fatal(format!(
            "no .ptk files found in workspace '{}'",
            dir.display()
        )));
    }

    // Sort for deterministic ordering
    paths.sort();

    let mut sources = SourceMap::new();
    let mut shebang = ShebangDecl::default();
    let mut lang_origin: Option<String> = None;
    let mut entry_origin: Option<String> = None;

    for path in paths {
        let content = read_file(&path)?;
        let display = path.display().to_string();
        let declared = extract_shebang(&content, &display)?;

        merge_declaration(
            "language",
            &mut shebang.lang,
            &mut lang_origin,
            declared.lang.map(|raw| normalize_lang_alias(&raw)),
            &display,
        )?;
        merge_declaration(
            "entrypoint",
            &mut shebang.entry,
            &mut entry_origin,
            declared.entry,
            &display,
        )?;

        let source_path = path.to_string_lossy();
        sources.add_file(SourcePath::new(&source_path), &content);
    }

    Ok(QuerySources { sources, shebang })
}

/// Normalize aliases (`ts` → `typescript`) so workspace agreement isn't
/// fooled by spelling. Unknown names stay raw; resolution errors surface later.
fn normalize_lang_alias(raw: &str) -> String {
    crate::language_registry::from_name(raw)
        .map(|l| l.name().to_string())
        .unwrap_or_else(|| raw.to_string())
}

fn read_file(path: &Path) -> Result<String, CliError> {
    fs::read_to_string(path)
        .map_err(|e| CliError::fatal(format!("failed to read '{}': {}", path.display(), e)))
}

fn extract_shebang(content: &str, origin: &str) -> Result<ShebangDecl, CliError> {
    match parse_shebang(content) {
        Ok(options) => Ok(options.unwrap_or_default()),
        Err(msg) => Err(CliError::fatal(format!(
            "invalid shebang in '{}': {}",
            origin, msg
        ))),
    }
}

fn merge_declaration(
    what: &str,
    merged: &mut Option<String>,
    origin: &mut Option<String>,
    declared: Option<String>,
    file: &str,
) -> Result<(), CliError> {
    let Some(value) = declared else {
        return Ok(());
    };

    match merged {
        Some(existing) if *existing != value => Err(CliError::fatal(format!(
            "workspace shebangs disagree on {}: '{}' in '{}' vs '{}' in '{}'",
            what,
            existing,
            origin.as_deref().unwrap_or("<unknown>"),
            value,
            file,
        ))),
        Some(_) => Ok(()),
        None => {
            *merged = Some(value);
            *origin = Some(file.to_string());
            Ok(())
        }
    }
}
