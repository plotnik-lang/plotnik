use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::corpus::{Case, SourceLanguage};
use crate::{generate, process};

struct RustCase<'a> {
    case: &'a Case,
    shard: String,
    name: String,
}

#[derive(serde::Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoPackage>,
    resolve: CargoResolve,
}

#[derive(serde::Deserialize)]
struct CargoPackage {
    id: String,
    name: String,
    manifest_path: PathBuf,
}

#[derive(serde::Deserialize)]
struct CargoResolve {
    nodes: Vec<CargoNode>,
}

#[derive(serde::Deserialize)]
struct CargoNode {
    id: String,
    deps: Vec<CargoDependency>,
}

#[derive(serde::Deserialize)]
struct CargoDependency {
    name: String,
    pkg: String,
}

pub(crate) fn generate(manifest_dir: &Path, plotnik: &Path, cases: &[Case]) -> Result<(), String> {
    let cases = prepare_cases(cases)?;
    let project = manifest_dir.join("codegen/rust");
    verify_grammar_dependencies(&project, &cases)?;
    generate_and_promote(&project, plotnik, &cases)
}

fn generate_and_promote(
    project: &Path,
    plotnik: &Path,
    cases: &[RustCase<'_>],
) -> Result<(), String> {
    let tests = project.join("tests");
    let pending = project.join(format!("tests.pending-{}", std::process::id()));
    remove_stale_staging(project)?;

    let result = stage(&pending, plotnik, cases);
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&pending);
        return Err(error);
    }
    if tests.exists() {
        fs::remove_dir_all(&tests)
            .map_err(|error| format!("remove previous generated tests: {error}"))?;
    }
    fs::rename(&pending, &tests)
        .map_err(|error| format!("promote generated tests into {}: {error}", tests.display()))?;
    Ok(())
}

fn remove_stale_staging(project: &Path) -> Result<(), String> {
    let entries = fs::read_dir(project)
        .map_err(|error| format!("read Rust codegen project {}: {error}", project.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!(
                "read Rust codegen project entry {}: {error}",
                project.display()
            )
        })?;
        let name = entry.file_name();
        if !name
            .to_str()
            .is_some_and(|name| name.starts_with("tests.pending-"))
        {
            continue;
        }
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("inspect stale staging {}: {error}", path.display()))?;
        if !file_type.is_dir() {
            return Err(format!(
                "stale staging path is not a directory: {}",
                path.display()
            ));
        }
        fs::remove_dir_all(&path)
            .map_err(|error| format!("remove stale staging {}: {error}", path.display()))?;
    }
    Ok(())
}

fn prepare_cases(cases: &[Case]) -> Result<Vec<RustCase<'_>>, String> {
    let mut names = BTreeMap::new();
    let mut prepared = Vec::with_capacity(cases.len());
    for case in cases {
        let (shard, name) = rust_names(&case.relative)?;
        let rust_test = format!("{shard}/{name}");
        if let Some(previous) = names.insert(rust_test.clone(), &case.relative) {
            return Err(format!(
                "snapshots `{previous}` and `{}` both normalize to Rust test `{rust_test}`",
                case.relative
            ));
        }
        prepared.push(RustCase { case, shard, name });
    }
    Ok(prepared)
}

fn stage(root: &Path, plotnik: &Path, cases: &[RustCase<'_>]) -> Result<(), String> {
    fs::create_dir_all(root.join("generated"))
        .map_err(|error| format!("create Rust staging tree: {error}"))?;
    let mut shards: BTreeMap<&str, Vec<&RustCase<'_>>> = BTreeMap::new();

    for case in cases {
        shards.entry(&case.shard).or_default().push(case);
        let directory = case_directory(root, case);
        fs::create_dir_all(&directory).map_err(|error| {
            format!("create snapshot directory {}: {error}", directory.display())
        })?;
        let query_path = directory.join("query.ptk");
        fs::write(&query_path, case.case.query.as_bytes())
            .map_err(|error| format!("write {}: {error}", query_path.display()))?;
        fs::write(directory.join("input"), case.case.source.as_bytes())
            .map_err(|error| format!("write input for `{}`: {error}", case.case.relative))?;
        fs::write(
            directory.join("expected"),
            case.case.expected.text().as_bytes(),
        )
        .map_err(|error| {
            format!(
                "write expected output for `{}`: {error}",
                case.case.relative
            )
        })?;
        let matcher = generate::source(plotnik, "rust", case.case, &query_path)?;
        fs::write(directory.join("matcher.rs"), matcher.as_bytes())
            .map_err(|error| format!("write matcher for `{}`: {error}", case.case.relative))?;
    }

    for (shard, cases) in shards {
        let source = render_shard(&cases);
        fs::write(root.join(format!("{shard}.rs")), source)
            .map_err(|error| format!("write Rust shard `{shard}`: {error}"))?;
    }
    Ok(())
}

fn case_directory(root: &Path, case: &RustCase<'_>) -> PathBuf {
    let stem = case
        .case
        .relative
        .strip_suffix(".txt")
        .expect("corpus snapshot paths end in .txt");
    root.join("generated").join(stem)
}

fn render_shard(cases: &[&RustCase<'_>]) -> String {
    let mut out = String::from("use plotnik_codegen_rust_tests::{Language, parse};\n\n");
    for (index, case) in cases.iter().enumerate() {
        let relative = case
            .case
            .relative
            .strip_suffix(".txt")
            .expect("corpus snapshot paths end in .txt");
        let module = format!("matcher_{index:03}");
        let _ = writeln!(
            out,
            "#[path = \"generated/{relative}/matcher.rs\"]\npub mod {module};"
        );
    }
    out.push('\n');
    for (index, case) in cases.iter().enumerate() {
        let relative = case
            .case
            .relative
            .strip_suffix(".txt")
            .expect("corpus snapshot paths end in .txt");
        let module = format!("matcher_{index:03}");
        let language = language_variant(case.case.language);
        let _ = writeln!(out, "#[test]");
        let _ = writeln!(out, "fn {}() {{", case.name);
        render_include(&mut out, "source", &format!("generated/{relative}/input"));
        render_include(
            &mut out,
            "expected",
            &format!("generated/{relative}/expected"),
        );
        let _ = writeln!(out, "    let tree = parse(Language::{language}, source);");
        let _ = writeln!(
            out,
            "    let actual = {module}::Q::parse_to_json(&tree, source)"
        );
        let _ = writeln!(
            out,
            "        .expect(\"snapshot must stay within generated runtime limits\")"
        );
        let _ = writeln!(
            out,
            "        .unwrap_or_else(|| \"<no match>\".to_string());"
        );
        let _ = writeln!(out);
        let _ = writeln!(out, "    assert_eq!(expected, actual);");
        let _ = writeln!(out, "}}\n");
    }
    let trimmed = out.trim_end().len();
    out.truncate(trimmed);
    out.push('\n');
    out
}

fn render_include(out: &mut String, binding: &str, path: &str) {
    let compact = format!("    let {binding} = include_str!({path:?});");
    if compact.len() <= 100 {
        let _ = writeln!(out, "{compact}");
        return;
    }
    let continued = format!("        include_str!({path:?});");
    if continued.len() <= 100 {
        let _ = writeln!(out, "    let {binding} =");
        let _ = writeln!(out, "{continued}");
        return;
    }
    let _ = writeln!(out, "    let {binding} = include_str!(");
    let _ = writeln!(out, "        {path:?}");
    let _ = writeln!(out, "    );");
}

fn verify_grammar_dependencies(project: &Path, cases: &[RustCase<'_>]) -> Result<(), String> {
    let mut command = Command::new("cargo");
    command
        .args(["metadata", "--locked", "--format-version", "1"])
        .current_dir(project);
    let raw = process::capture(&mut command, "resolve native Rust grammar dependencies")?;
    let metadata: CargoMetadata = serde_json::from_slice(&raw)
        .map_err(|error| format!("parse native Rust cargo metadata: {error}"))?;
    let project_manifest = fs::canonicalize(project.join("Cargo.toml")).map_err(|error| {
        format!(
            "resolve native Rust project manifest {}: {error}",
            project.join("Cargo.toml").display()
        )
    })?;

    let mut expected = BTreeMap::new();
    for case in cases {
        let root = grammar_package_root(&case.case.grammar_json)?;
        expected.insert(
            dependency_name(case.case.language),
            (
                package_name(case.case.language),
                fs::canonicalize(root).map_err(|error| {
                    format!("resolve grammar package root {}: {error}", root.display())
                })?,
            ),
        );
    }

    for (dependency_name, (package_name, expected_root)) in expected {
        let package = direct_dependency(&metadata, &project_manifest, dependency_name)?;
        if package.name != package_name {
            return Err(format!(
                "native Rust dependency `{dependency_name}` resolved unexpected package `{}`",
                package.name
            ));
        }
        let manifest_root = package
            .manifest_path
            .parent()
            .expect("Cargo package manifests have a parent directory");
        let actual_root = fs::canonicalize(manifest_root).map_err(|error| {
            format!(
                "resolve native `{package_name}` root {}: {error}",
                manifest_root.display()
            )
        })?;
        if actual_root == expected_root {
            continue;
        }
        return Err(format!(
            "grammar package mismatch for `{package_name}`: `plotnik gen` used {}, native Rust resolved {}",
            expected_root.display(),
            actual_root.display()
        ));
    }
    Ok(())
}

fn direct_dependency<'a>(
    metadata: &'a CargoMetadata,
    project_manifest: &Path,
    dependency_name: &str,
) -> Result<&'a CargoPackage, String> {
    let project = metadata
        .packages
        .iter()
        .find(|package| package.manifest_path.as_path() == project_manifest)
        .ok_or_else(|| {
            format!(
                "native Rust project is absent from cargo metadata: {}",
                project_manifest.display()
            )
        })?;
    let node = metadata
        .resolve
        .nodes
        .iter()
        .find(|node| node.id == project.id)
        .ok_or_else(|| "native Rust project is absent from cargo's resolve graph".to_string())?;
    let dependency = node
        .deps
        .iter()
        .find(|dependency| dependency.name == dependency_name)
        .ok_or_else(|| {
            format!("native Rust project does not resolve direct dependency `{dependency_name}`")
        })?;
    metadata
        .packages
        .iter()
        .find(|package| package.id == dependency.pkg)
        .ok_or_else(|| {
            format!(
                "resolved package `{}` for direct dependency `{dependency_name}` is absent from cargo metadata",
                dependency.pkg
            )
        })
}

fn grammar_package_root(grammar_json: &Path) -> Result<&Path, String> {
    grammar_json.ancestors().nth(3).ok_or_else(|| {
        format!(
            "grammar path does not have `<package>/grammar/src/grammar.json` shape: {}",
            grammar_json.display()
        )
    })
}

fn rust_names(relative: &str) -> Result<(String, String), String> {
    let stem = relative
        .strip_suffix(".txt")
        .ok_or_else(|| format!("snapshot path does not end in `.txt`: {relative}"))?;
    let components = stem.split('/').collect::<Vec<_>>();
    if components.len() < 2 {
        return Err(format!(
            "snapshot path must contain a Rust shard and test name: {relative}"
        ));
    }
    let shard = components
        .first()
        .ok_or_else(|| format!("snapshot path has no shard: {relative}"))?
        .to_string();
    if !components.iter().all(|component| is_snake_case(component)) {
        return Err(format!(
            "snapshot path components must be snake_case for Rust: {relative}"
        ));
    }
    Ok((shard, format!("snapshot_{}", components[1..].join("__"))))
}

fn is_snake_case(value: &str) -> bool {
    let mut bytes = value.bytes();
    let valid_start = bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_lowercase() || byte == b'_');
    valid_start
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn language_variant(language: SourceLanguage) -> &'static str {
    match language {
        SourceLanguage::JavaScript => "JavaScript",
        SourceLanguage::TypeScript => "TypeScript",
        SourceLanguage::Dart => "Dart",
    }
}

fn package_name(language: SourceLanguage) -> &'static str {
    match language {
        SourceLanguage::JavaScript => "arborium-javascript",
        SourceLanguage::TypeScript => "arborium-typescript",
        SourceLanguage::Dart => "arborium-dart",
    }
}

fn dependency_name(language: SourceLanguage) -> &'static str {
    match language {
        SourceLanguage::JavaScript => "arborium_javascript",
        SourceLanguage::TypeScript => "arborium_typescript",
        SourceLanguage::Dart => "arborium_dart",
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use crate::corpus::Expected;

    use super::*;

    static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn nested_paths_have_stable_rust_names() {
        assert_eq!(
            rust_names("captures/nested/named_node.txt").unwrap(),
            (
                "captures".to_string(),
                "snapshot_nested__named_node".to_string()
            )
        );
    }

    #[test]
    fn rust_test_names_are_valid_when_snapshot_names_are_keywords() {
        assert_eq!(
            rust_names("captures/match.txt").unwrap(),
            ("captures".to_string(), "snapshot_match".to_string())
        );
    }

    #[test]
    fn shard_uses_q_and_native_string_assertion() {
        let cases = [case("captures/named_node.txt")];
        let prepared = prepare_cases(&cases).unwrap();
        let shard = render_shard(&prepared.iter().collect::<Vec<_>>());

        assert!(shard.contains("matcher_000::Q::parse_to_json(&tree, source)"));
        assert!(shard.contains("fn snapshot_named_node()"));
        assert!(shard.contains("assert_eq!(expected, actual);"));
        assert!(!shard.contains("serde_json"));
    }

    #[test]
    fn failed_generation_preserves_previous_tests_and_removes_all_staging() {
        let root = temp_root("atomic-stage");
        let project = root.join("codegen/rust");
        let tests = project.join("tests");
        let pending = project.join(format!("tests.pending-{}", std::process::id()));
        let interrupted = project.join("tests.pending-previous-process");
        fs::create_dir_all(&tests).unwrap();
        fs::create_dir_all(&pending).unwrap();
        fs::create_dir_all(&interrupted).unwrap();
        fs::write(tests.join("marker"), "previous").unwrap();
        fs::write(pending.join("stale"), "stale").unwrap();
        fs::write(interrupted.join("stale"), "stale").unwrap();

        let cases = [case("captures/named_node.txt")];
        let prepared = prepare_cases(&cases).unwrap();
        let error =
            generate_and_promote(&project, &root.join("missing-plotnik"), &prepared).unwrap_err();

        assert!(error.contains("failed to start"));
        assert_eq!(
            fs::read_to_string(tests.join("marker")).unwrap(),
            "previous"
        );
        assert!(!pending.exists());
        assert!(!interrupted.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn grammar_resolution_follows_the_direct_dependency_edge() {
        let project_manifest = PathBuf::from("/workspace/codegen-rust/Cargo.toml");
        let metadata = CargoMetadata {
            packages: vec![
                package("root", "plotnik-codegen-rust-tests", &project_manifest),
                package(
                    "wrong",
                    "arborium-javascript",
                    Path::new("/registry/arborium-javascript-1/Cargo.toml"),
                ),
                package(
                    "right",
                    "arborium-javascript",
                    Path::new("/registry/arborium-javascript-2/Cargo.toml"),
                ),
            ],
            resolve: CargoResolve {
                nodes: vec![CargoNode {
                    id: "root".to_string(),
                    deps: vec![CargoDependency {
                        name: "arborium_javascript".to_string(),
                        pkg: "right".to_string(),
                    }],
                }],
            },
        };

        let resolved =
            direct_dependency(&metadata, &project_manifest, "arborium_javascript").unwrap();

        assert_eq!(resolved.id, "right");
    }

    fn case(relative: &str) -> Case {
        Case {
            relative: relative.to_string(),
            query: "Q = (program)".to_string(),
            source: "x".to_string(),
            language: SourceLanguage::JavaScript,
            grammar_json: PathBuf::from("grammar/src/grammar.json"),
            expected: Expected::Json("null".to_string()),
        }
    }

    fn temp_root(label: &str) -> PathBuf {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "plotnik-codegen-{label}-{}-{sequence}",
            std::process::id()
        ))
    }

    fn package(id: &str, name: &str, manifest_path: &Path) -> CargoPackage {
        CargoPackage {
            id: id.to_string(),
            name: name.to_string(),
            manifest_path: manifest_path.to_path_buf(),
        }
    }
}
