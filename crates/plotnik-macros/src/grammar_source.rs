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

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use plotnik_lib::grammar::Grammar;
use plotnik_lib::grammar::raw::RawGrammar;

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

impl ResolvedGrammar {
    fn read(path: PathBuf) -> Result<Self, String> {
        let json = read(&path)?;
        Ok(Self { json, path })
    }
}

pub struct LoadedGrammar {
    pub grammar: Arc<Grammar>,
    pub path: PathBuf,
}

struct NamedGrammar {
    name: String,
    resolved: ResolvedGrammar,
}

impl NamedGrammar {
    fn read(path: PathBuf) -> Result<Self, String> {
        let resolved = ResolvedGrammar::read(path)?;
        let name = grammar_name(&resolved.json).ok_or_else(|| {
            format!(
                "`{}` has no top-level `name` field; not a tree-sitter grammar.json?",
                resolved.path.display()
            )
        })?;
        Ok(Self { name, resolved })
    }
}

pub fn resolve(spec: &GrammarSpec<'_>, base_dir: Option<&Path>) -> Result<ResolvedGrammar, String> {
    match spec {
        GrammarSpec::Path(raw) => {
            let path = resolve_relative(raw, base_dir)?;
            ResolvedGrammar::read(path)
        }
        GrammarSpec::Package { name, subgrammar } => resolve_package(name, *subgrammar),
    }
}

/// Resolve and parse the grammar, cached by resolved path for the life of
/// the rustc process. Expansion is not incremental — every check of the
/// using crate re-runs every `query!` in it — and a grammar.json can be
/// megabytes (tree-sitter-typescript), so re-parsing it once per invocation
/// is real build time. Files are stable within one compiler process; edits
/// between processes are what the `include_bytes!` rebuild anchors track.
pub fn load(spec: &GrammarSpec<'_>, base_dir: Option<&Path>) -> Result<LoadedGrammar, String> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Arc<Grammar>>>> = OnceLock::new();

    let resolved = resolve(spec, base_dir)?;
    let cache = CACHE.get_or_init(Mutex::default);
    if let Some(grammar) = cache
        .lock()
        .expect("grammar cache lock is never poisoned")
        .get(&resolved.path)
    {
        return Ok(LoadedGrammar {
            grammar: Arc::clone(grammar),
            path: resolved.path,
        });
    }

    let raw = RawGrammar::from_json(&resolved.json)
        .map_err(|error| format!("invalid grammar `{}`: {error}", resolved.path.display()))?;
    let grammar = Grammar::from_raw(&raw)
        .map_err(|error| format!("invalid grammar `{}`: {error}", resolved.path.display()))?;
    let grammar = Arc::new(grammar);
    cache
        .lock()
        .expect("grammar cache lock is never poisoned")
        .insert(resolved.path.clone(), Arc::clone(&grammar));
    Ok(LoadedGrammar {
        grammar,
        path: resolved.path,
    })
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
    let package = select_dependency_package(metadata, name)?;
    let candidates = package_grammar_jsons(package, name)?;

    // One grammar and no selector: done. Everything else goes through the
    // `name` field inside each grammar.json, the only spelling that is
    // stable across crate layouts.
    if candidates.len() == 1 && subgrammar.is_none() {
        let path = candidates.into_iter().next().expect("checked non-empty");
        return ResolvedGrammar::read(path);
    }

    let named = candidates
        .into_iter()
        .map(NamedGrammar::read)
        .collect::<Result<Vec<_>, _>>()?;
    resolve_named_grammar(name, subgrammar, named)
}

fn select_dependency_package<'a>(
    metadata: &'a cargo_metadata::Metadata,
    name: &str,
) -> Result<&'a cargo_metadata::Package, String> {
    let closure = dependency_closure(metadata);
    let packages: Vec<&cargo_metadata::Package> = metadata
        .packages
        .iter()
        .filter(|package| package.name.as_str() == name)
        .filter(|package| {
            closure
                .as_ref()
                .is_none_or(|reachable| reachable.contains(&package.id))
        })
        .collect();

    match packages.as_slice() {
        [] => Err(format!(
            "package `{name}` is not in this crate's dependency graph; add it \
                 to [dependencies] — any crate that ships a grammar.json works \
                 (tree-sitter-*, arborium-*, or your own grammar crate)"
        )),
        [package] => Ok(*package),
        several => {
            let versions = several
                .iter()
                .map(|package| package.version.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            Err(format!(
                "several versions of `{name}` are in the dependency graph \
                 ({versions}); the grammar to bake is ambiguous"
            ))
        }
    }
}

fn package_grammar_jsons(
    package: &cargo_metadata::Package,
    name: &str,
) -> Result<Vec<PathBuf>, String> {
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
    Ok(candidates)
}

fn resolve_named_grammar(
    name: &str,
    subgrammar: Option<&str>,
    named: Vec<NamedGrammar>,
) -> Result<ResolvedGrammar, String> {
    let Some(wanted) = subgrammar else {
        let names = named
            .iter()
            .map(|grammar| grammar.name.as_str())
            .collect::<Vec<_>>()
            .join("`, `");
        return Err(format!(
            "package `{name}` ships several grammars (`{names}`); pick one with \
             `grammar = \"{name}/{}\"`",
            named[0].name
        ));
    };

    match named.into_iter().find(|grammar| grammar.name == wanted) {
        Some(grammar) => Ok(grammar.resolved),
        None => Err(format!(
            "package `{name}` ships no grammar named `{wanted}`"
        )),
    }
}

/// Package ids reachable from the invoking crate in the resolved dependency
/// graph — the set `grammar = "package"` may name; the whole workspace graph
/// would also admit packages only some *other* workspace member depends on.
/// `None` when the walk isn't possible (no resolve section, invoking package
/// not identifiable): the caller then falls back to the whole graph, and the
/// generated module's language-skew check still backstops a wrong pick.
fn dependency_closure(
    metadata: &cargo_metadata::Metadata,
) -> Option<std::collections::HashSet<&cargo_metadata::PackageId>> {
    let resolve = metadata.resolve.as_ref()?;
    let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") else {
        return None;
    };
    let manifest = Path::new(&manifest_dir).join("Cargo.toml");
    let root = metadata
        .packages
        .iter()
        .find(|package| package.manifest_path.as_std_path() == manifest)?;

    let nodes: HashMap<&cargo_metadata::PackageId, &cargo_metadata::Node> =
        resolve.nodes.iter().map(|node| (&node.id, node)).collect();
    let mut reachable = std::collections::HashSet::new();
    let mut stack = vec![&root.id];
    while let Some(id) = stack.pop() {
        if !reachable.insert(id) {
            continue;
        }
        if let Some(node) = nodes.get(id) {
            stack.extend(node.deps.iter().map(|dep| &dep.pkg));
        }
    }
    Some(reachable)
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

            if !path.is_dir() {
                if name == "grammar.json" {
                    found.push(path);
                }
                continue;
            }

            if is_ignored_package_dir(&name) {
                continue;
            }

            walk(&path, depth + 1, found);
        }
    }

    let mut found = Vec::new();
    walk(root, 0, &mut found);
    found.sort();
    found
}

fn is_ignored_package_dir(name: &str) -> bool {
    name.starts_with('.') || name == "node_modules" || name == "target"
}

/// The `name` field of a grammar.json — how subgrammars are told apart.
fn grammar_name(json: &str) -> Option<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return None;
    };
    let name = value.get("name").and_then(|name| name.as_str())?;
    Some(name.to_string())
}
