//! Where a query's grammar comes from — the pluggable half of `query!`.
//!
//! There is deliberately no built-in language registry and no per-language
//! feature flags here: the caller's own dependency graph is the registry.
//! Any package that ships a `grammar.json` works — `tree-sitter-*` crates,
//! `arborium-*` crates, or a local grammar crate — and the grammar version
//! is exactly the package version the caller's lockfile resolved, i.e. the
//! same package whose parser they link at runtime. A filesystem path is the
//! escape hatch for grammars that live outside the graph.
//!
//! New source kinds (an env override, a workspace-level grammar map, ...)
//! are one more [`GrammarSpec`] variant plus a [`resolve`] arm.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Debug, PartialEq)]
pub enum GrammarSpec<'a> {
    /// A `grammar.json` path, resolved like `include_str!` (relative to the
    /// invoking file).
    Path(&'a str),
    /// A package in the caller's dependency graph, optionally narrowed to
    /// one of several grammars it ships (`"tree-sitter-typescript/tsx"`).
    Package {
        name: &'a str,
        subgrammar: Option<&'a str>,
    },
}

/// Classify the `grammar = "..."` argument. Only a `.json` suffix means a
/// filesystem path — `/` alone is the package/subgrammar separator.
pub fn parse_spec(raw: &str) -> GrammarSpec<'_> {
    if raw.ends_with(".json") {
        return GrammarSpec::Path(raw);
    }
    match raw.split_once('/') {
        Some((name, sub)) => GrammarSpec::Package {
            name,
            subgrammar: Some(sub),
        },
        None => GrammarSpec::Package {
            name: raw,
            subgrammar: None,
        },
    }
}

#[derive(Debug)]
pub struct ResolvedGrammar {
    pub json: String,
    /// Absolute path of the `grammar.json` actually read; the expansion
    /// anchors an `include_bytes!` on it so in-place edits (path
    /// dependencies, local grammars) retrigger the macro.
    pub path: PathBuf,
}

pub fn resolve(spec: &GrammarSpec<'_>, base_dir: Option<&Path>) -> Result<ResolvedGrammar, String> {
    match spec {
        GrammarSpec::Path(raw) => {
            let path = resolve_relative(raw, base_dir)?;
            let json = read(&path)?;
            Ok(ResolvedGrammar { json, path })
        }
        GrammarSpec::Package { name, subgrammar } => resolve_package(name, *subgrammar),
    }
}

/// Resolve a user-written path the way `include_str!` would: absolute paths
/// as-is, relative ones against the invoking file's directory.
pub fn resolve_relative(raw: &str, base_dir: Option<&Path>) -> Result<PathBuf, String> {
    let path = Path::new(raw);
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    let Some(base) = base_dir else {
        return Err(format!(
            "cannot resolve the relative path `{raw}`: the invoking file's \
             location is unknown here; use an absolute path"
        ));
    };
    Ok(base.join(path))
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path)
        .map_err(|error| format!("failed to read `{}`: {error}", path.display()))
}

fn resolve_package(name: &str, subgrammar: Option<&str>) -> Result<ResolvedGrammar, String> {
    let metadata = metadata()?;
    let packages: Vec<&cargo_metadata::Package> = metadata
        .packages
        .iter()
        .filter(|package| package.name.as_str() == name)
        .collect();

    let package = match packages.as_slice() {
        [] => {
            return Err(format!(
                "package `{name}` is not in this crate's dependency graph; add it \
                 to [dependencies] — any crate that ships a grammar.json works \
                 (tree-sitter-*, arborium-*, or your own grammar crate)"
            ));
        }
        [package] => package,
        several => {
            let versions = several
                .iter()
                .map(|package| package.version.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(format!(
                "several versions of `{name}` are in the dependency graph \
                 ({versions}); the grammar to bake is ambiguous"
            ));
        }
    };

    let root = package
        .manifest_path
        .parent()
        .expect("a package manifest always has a parent directory");
    let candidates = find_grammar_jsons(root.as_std_path());
    if candidates.is_empty() {
        return Err(format!(
            "package `{name}` ({}) ships no grammar.json; pass a \
             `grammar = \"path/to/grammar.json\"` instead",
            package.version
        ));
    }

    // One grammar and no selector: done. Everything else goes through the
    // `name` field inside each grammar.json, the only spelling that is
    // stable across crate layouts.
    if candidates.len() == 1 && subgrammar.is_none() {
        let path = candidates.into_iter().next().expect("checked non-empty");
        let json = read(&path)?;
        return Ok(ResolvedGrammar { json, path });
    }

    let mut named = Vec::new();
    for path in candidates {
        let json = read(&path)?;
        let grammar_name = grammar_name(&json).ok_or_else(|| {
            format!(
                "`{}` has no top-level `name` field; not a tree-sitter grammar.json?",
                path.display()
            )
        })?;
        named.push((grammar_name, json, path));
    }

    let Some(wanted) = subgrammar else {
        let names = named
            .iter()
            .map(|(grammar_name, ..)| grammar_name.as_str())
            .collect::<Vec<_>>()
            .join("`, `");
        return Err(format!(
            "package `{name}` ships several grammars (`{names}`); pick one with \
             `grammar = \"{name}/{}\"`",
            named[0].0
        ));
    };

    match named
        .into_iter()
        .find(|(grammar_name, ..)| grammar_name == wanted)
    {
        Some((_, json, path)) => Ok(ResolvedGrammar { json, path }),
        None => Err(format!(
            "package `{name}` ships no grammar named `{wanted}`"
        )),
    }
}

/// The caller's resolved dependency graph, computed once per rustc process.
/// `cargo metadata` re-reads the workspace's own lockfile, so during a build
/// it neither resolves anew nor touches the network.
fn metadata() -> Result<&'static cargo_metadata::Metadata, String> {
    static METADATA: OnceLock<Result<cargo_metadata::Metadata, String>> = OnceLock::new();
    METADATA
        .get_or_init(|| {
            let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").map_err(|_| {
                "CARGO_MANIFEST_DIR is not set; `query!` needs cargo to locate \
                 grammar packages"
                    .to_string()
            })?;
            cargo_metadata::MetadataCommand::new()
                .manifest_path(Path::new(&manifest_dir).join("Cargo.toml"))
                .exec()
                .map_err(|error| format!("failed to run `cargo metadata`: {error}"))
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// Every `grammar.json` inside a package checkout. Grammar files sit shallow
/// (`src/`, `grammar/src/`, `<subgrammar>/src/`), so the walk is depth-capped
/// and skips directories that can only contain noise.
fn find_grammar_jsons(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: usize, found: &mut Vec<PathBuf>) {
        if depth > 4 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if path.is_dir() {
                if name.starts_with('.') || name == "node_modules" || name == "target" {
                    continue;
                }
                walk(&path, depth + 1, found);
            } else if name == "grammar.json" {
                found.push(path);
            }
        }
    }

    let mut found = Vec::new();
    walk(root, 0, &mut found);
    found.sort();
    found
}

/// The `name` field of a grammar.json — how subgrammars are told apart.
fn grammar_name(json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    Some(value.get("name")?.as_str()?.to_string())
}
