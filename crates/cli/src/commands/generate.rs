use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use plotnik_compiler::{
    CodegenProvenance, DiagnosticKind, Diagnostics, RustCodegenConfig, SourceCollection, SourceId,
    SourceMap, parse_grammar_declaration,
};

use super::compile::{compile_query, report_diagnostics};
use super::language::LanguageOptions;
use super::query_loader::load_workspace;
use crate::error::{CliError, CliResult, writeln_stderr};

const OWNERSHIP_HEADER: &str = "// Generated. Do not edit.\n";

pub struct GenerateArgs {
    pub query_path: PathBuf,
    pub language: LanguageOptions,
    pub color: bool,
}

pub fn run(args: GenerateArgs) -> CliResult {
    let folder = &args.query_path.components().collect::<PathBuf>();
    let metadata = fs::symlink_metadata(folder).map_err(|error| io_error(folder, error))?;
    if !metadata.is_dir() {
        return Err(CliError::fatal(format!(
            "expected a directory, not a file or symlink: '{}'",
            folder.display()
        )));
    }

    let sources = load_workspace(folder)?.sources;
    if sources.is_empty() {
        return Err(CliError::fatal(
            "nothing to generate: no immediate .ptk files",
        ));
    }

    let selections = group_sources(&sources, &args.language, args.color)?;
    let mut groups = BTreeMap::new();
    for (selection, ids) in selections {
        let artifacts = args.language.load_selected(&selection)?;
        let identity = artifacts
            .grammar
            .identity()
            .expect("loaded grammar artifacts have an identity");
        let key = (identity.name().to_owned(), identity.sha256().to_owned());
        let (_, sources) = groups
            .entry(key)
            .or_insert_with(|| (artifacts.grammar, Vec::new()));
        sources.extend(ids);
    }
    let mut outputs = BTreeMap::new();
    let mut modules = BTreeSet::new();
    for (_, (grammar, ids)) in groups {
        // Explicit membership prevents untagged files leaking into other groups.
        let mut collection = SourceCollection::new(sources.clone());
        for id in ids {
            collection.include(id);
        }
        let compiled = compile_query(collection, &grammar, args.color)?;
        let emission = compiled
            .emit_rust_sources(RustCodegenConfig::new().provenance(CodegenProvenance::Full))
            .map_err(|error| CliError::fatal(error.to_string()))?;
        report_diagnostics(emission.diagnostics(), compiled.source_map(), args.color)?;
        for output in emission
            .into_artifact()
            .expect("valid queries emit Rust source modules")
        {
            modules.insert(output.module_name().to_owned());
            let path = Path::new(
                compiled
                    .source_map()
                    .path(output.source_id())
                    .expect("folder sources have file paths"),
            )
            .with_extension("rs");
            let nested_index = path.with_extension("").join("mod.rs");
            if nested_index
                .try_exists()
                .map_err(|error| io_error(&nested_index, error))?
            {
                return Err(CliError::fatal(format!(
                    "conflicting Rust modules: '{}' and '{}'",
                    path.display(),
                    nested_index.display()
                )));
            }
            outputs.insert(path, format!("{OWNERSHIP_HEADER}{}", output.source()));
        }
    }

    let declarations = modules
        .iter()
        .map(|name| format!("pub mod {name};\n"))
        .collect::<String>();
    let index = folder.join("mod.rs");
    // Resolve '.' as well as named folder arguments before finding a sibling index.
    let absolute_folder = fs::canonicalize(folder).map_err(|error| io_error(folder, error))?;
    let mut sibling_name = absolute_folder.as_os_str().to_os_string();
    sibling_name.push(".rs");
    let sibling = PathBuf::from(sibling_name);
    let sibling_exists = file_metadata(&sibling)?.is_some();
    let index_content = read_existing(&index)?;
    if sibling_exists && index_content.is_some() {
        return Err(CliError::fatal(format!(
            "conflicting Rust module indexes: '{}' and '{}'",
            sibling.display(),
            index.display()
        )));
    }
    let manual_index = if sibling_exists {
        Some(sibling)
    } else if index_content
        .as_ref()
        .is_some_and(|(content, _)| !content.starts_with(OWNERSHIP_HEADER.as_bytes()))
    {
        Some(index.clone())
    } else {
        outputs.insert(index, format!("{OWNERSHIP_HEADER}{declarations}"));
        None
    };

    // Validate every destination before staging or replacing any generated file.
    let mut changed = Vec::new();
    for (path, content) in outputs {
        let permissions = if let Some((existing, permissions)) = read_existing(&path)? {
            if !existing.starts_with(OWNERSHIP_HEADER.as_bytes()) {
                return Err(CliError::fatal(format!(
                    "refusing to overwrite handwritten Rust file '{}'",
                    path.display()
                )));
            }
            if existing == content.as_bytes() {
                continue;
            }

            Some(permissions)
        } else {
            None
        };
        changed.push((path, content, permissions));
    }

    let mut staged = Vec::new();
    for (path, content, permissions) in changed {
        let mut temporary = tempfile::Builder::new()
            .make_in(folder, |path| {
                fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create_new(true)
                    .open(path)
            })
            .map_err(|error| io_error(&path, error))?;
        temporary
            .write_all(content.as_bytes())
            .map_err(|error| io_error(&path, error))?;
        if let Some(permissions) = permissions {
            temporary
                .as_file()
                .set_permissions(permissions)
                .map_err(|error| io_error(&path, error))?;
        }
        staged.push((path, temporary));
    }
    for (path, temporary) in staged {
        temporary
            .persist(&path)
            .map_err(|error| io_error(&path, error.error))?;
    }
    if let Some(index) = manual_index {
        writeln_stderr(format_args!(
            "Module index '{}' left unchanged. Add any missing declarations:\n{declarations}",
            index.display()
        ))?;
    }
    Ok(())
}

fn group_sources(
    sources: &SourceMap,
    language: &LanguageOptions,
    color: bool,
) -> Result<BTreeMap<String, Vec<SourceId>>, CliError> {
    let mut groups: BTreeMap<String, Vec<SourceId>> = BTreeMap::new();
    let mut diagnostics = Diagnostics::new();
    let mut missing = Vec::new();
    for source in sources.iter() {
        let selection = match parse_grammar_declaration(source.clone()) {
            Ok(Some(tag)) => tag.language,
            Ok(None) => {
                let Some(fallback) = language.lang.as_deref() else {
                    missing.push(sources.path(source.id).expect("folder source has a path"));
                    continue;
                };
                fallback
            }
            Err(error) => {
                diagnostics
                    .report(DiagnosticKind::InvalidGrammarDeclaration, error.span)
                    .hint(error.message)
                    .emit();
                continue;
            }
        };
        groups
            .entry(selection.to_owned())
            .or_default()
            .push(source.id);
    }
    for path in &missing {
        writeln_stderr(format_args!(
            "error: '{path}' has no grammar tag. Set --lang or add a first-line tag."
        ))?;
    }
    report_diagnostics(&diagnostics, sources, color)?;
    if !missing.is_empty() {
        return Err(CliError::No);
    }
    Ok(groups)
}

fn file_metadata(path: &Path) -> Result<Option<fs::Metadata>, CliError> {
    // Symlinks do not establish ownership, even if their target is generated.
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() => Err(CliError::fatal(format!(
            "refusing non-file generation destination '{}'",
            path.display()
        ))),
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error(path, error)),
    }
}

fn read_existing(path: &Path) -> Result<Option<(Vec<u8>, fs::Permissions)>, CliError> {
    let Some(metadata) = file_metadata(path)? else {
        return Ok(None);
    };

    fs::read(path)
        .map(|content| Some((content, metadata.permissions())))
        .map_err(|error| io_error(path, error))
}

fn io_error(path: &Path, error: std::io::Error) -> CliError {
    CliError::fatal(format!("cannot access '{}': {error}", path.display()))
}
