use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use plotnik_core::grammar::compat::{
    MetadataCompatibilityResult, MetadataDifference, compare_metadata_lowering,
};
use plotnik_core::grammar::raw::RawGrammar;

const SKIP_REGISTRY_PACKAGES: &[&str] = &[
    "arborium-tree-sitter",
    "arborium-sysroot",
    "arborium-test-harness",
    "arborium-highlight",
    "arborium-host",
    "arborium-mdbook",
    "arborium-plugin-runtime",
    "arborium-rustdoc",
    "arborium-theme",
    "arborium-wire",
];

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<bool, String> {
    let paths = input_paths()?;
    let mut ok = true;

    for path in paths {
        ok &= check_path(&path);
    }

    Ok(ok)
}

fn input_paths() -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    let mut saw_arg = false;
    let mut args = env::args_os().skip(1);

    while let Some(arg) = args.next() {
        saw_arg = true;

        if arg == "--registry" {
            paths.extend(registry_paths()?);
            continue;
        }

        if arg == "--paths-file" {
            let Some(path) = args.next() else {
                return Err(format!("missing FILE after --paths-file\n\n{}", usage()));
            };
            paths.extend(paths_from_file(&PathBuf::from(path))?);
            continue;
        }

        if arg == "-h" || arg == "--help" {
            return Err(usage());
        }

        if looks_like_option(&arg) {
            return Err(format!(
                "unknown argument: {}\n\n{}",
                PathBuf::from(arg).display(),
                usage()
            ));
        }

        paths.push(PathBuf::from(arg));
    }

    if !saw_arg {
        return Err(usage());
    }

    if paths.is_empty() {
        return Err("no grammar.json inputs found".to_string());
    }

    Ok(paths)
}

fn looks_like_option(arg: &OsString) -> bool {
    arg.to_string_lossy().starts_with('-')
}

fn paths_from_file(path: &Path) -> Result<Vec<PathBuf>, String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("read paths file {}: {error}", path.display()))?;
    Ok(contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .collect())
}

fn registry_paths() -> Result<Vec<PathBuf>, String> {
    let home = env::var_os("HOME").ok_or_else(|| "HOME is not set".to_string())?;
    let registry_src = PathBuf::from(home).join(".cargo/registry/src");
    let registry_dirs = fs::read_dir(&registry_src)
        .map_err(|error| format!("read {}: {error}", registry_src.display()))?;
    let mut paths = Vec::new();

    for registry_dir in registry_dirs {
        let registry_dir = registry_dir
            .map_err(|error| format!("read {} entry: {error}", registry_src.display()))?;
        let file_type = registry_dir
            .file_type()
            .map_err(|error| format!("stat {}: {error}", registry_dir.path().display()))?;
        if !file_type.is_dir() {
            continue;
        }

        let package_dirs = fs::read_dir(registry_dir.path())
            .map_err(|error| format!("read {}: {error}", registry_dir.path().display()))?;
        for package_dir in package_dirs {
            let package_dir = package_dir.map_err(|error| {
                format!("read {} entry: {error}", registry_dir.path().display())
            })?;
            let file_type = package_dir
                .file_type()
                .map_err(|error| format!("stat {}: {error}", package_dir.path().display()))?;
            if !file_type.is_dir() {
                continue;
            }

            let dir_name = package_dir.file_name().to_string_lossy().into_owned();
            let package_name = registry_package_name(&dir_name);
            if !package_name.starts_with("arborium-")
                || SKIP_REGISTRY_PACKAGES.contains(&package_name)
            {
                continue;
            }

            let grammar_path = package_dir.path().join("grammar/src/grammar.json");
            if grammar_path.is_file() {
                paths.push(grammar_path);
            }
        }
    }

    paths.sort();
    Ok(paths)
}

fn registry_package_name(dir_name: &str) -> &str {
    for (index, _) in dir_name.match_indices('-') {
        let Some(next) = dir_name[index + 1..].chars().next() else {
            continue;
        };
        if next.is_ascii_digit() {
            return &dir_name[..index];
        }
    }

    dir_name
}

fn check_path(path: &Path) -> bool {
    let raw = match read_raw(path) {
        Ok(raw) => raw,
        Err(error) => {
            println!("{}: error {error}", path.display());
            return false;
        }
    };

    match compare_metadata_lowering(&raw).result {
        MetadataCompatibilityResult::Match {
            node_symbols,
            fields,
        } => {
            println!(
                "{}: ok {} ({} nodes, {} fields)",
                path.display(),
                raw.name,
                node_symbols,
                fields
            );
            true
        }
        MetadataCompatibilityResult::Mismatch {
            node_symbols,
            fields,
            differences,
        } => {
            println!(
                "{}: mismatch {} (nodes {}/{}, fields {}/{}) {}",
                path.display(),
                raw.name,
                node_symbols.metadata_only,
                node_symbols.full,
                fields.metadata_only,
                fields.full,
                format_differences(&differences)
            );
            false
        }
        MetadataCompatibilityResult::Error {
            metadata_only,
            full,
        } => {
            println!(
                "{}: error {} {}",
                path.display(),
                raw.name,
                format_lowering_errors(metadata_only.as_deref(), full.as_deref())
            );
            false
        }
    }
}

fn read_raw(path: &Path) -> Result<RawGrammar, String> {
    let json = fs::read_to_string(path).map_err(|error| format!("read: {error}"))?;
    RawGrammar::from_json(&json).map_err(|error| format!("parse: {error}"))
}

fn format_differences(differences: &[MetadataDifference]) -> String {
    differences
        .iter()
        .map(|difference| {
            format!(
                "{}[{}] metadata-only {} != full {}",
                difference.section,
                difference.index,
                difference.metadata_only.as_deref().unwrap_or("<missing>"),
                difference.full.as_deref().unwrap_or("<missing>")
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn format_lowering_errors(metadata_only: Option<&str>, full: Option<&str>) -> String {
    let mut parts = Vec::new();
    if let Some(error) = metadata_only {
        parts.push(format!("metadata-only: {error}"));
    }
    if let Some(error) = full {
        parts.push(format!("full: {error}"));
    }

    parts.join("; ")
}

fn usage() -> String {
    "usage: cargo run --manifest-path examples/abi-compat/Cargo.toml -- [--registry] [--paths-file FILE] <grammar.json> [...]".to_string()
}
