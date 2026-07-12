use std::path::PathBuf;

const GRAMMARS: &[(&str, &str)] = &[
    ("arborium_javascript", "JAVASCRIPT"),
    ("arborium_typescript", "TYPESCRIPT"),
    ("arborium_dart", "DART"),
];

fn main() {
    let manifest_dir =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set"));
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(manifest_dir.join("Cargo.toml"))
        .exec()
        .expect("test dependencies must resolve");
    let package = metadata
        .packages
        .iter()
        .find(|package| package.manifest_path.as_std_path() == manifest_dir.join("Cargo.toml"))
        .expect("plotnik-tests package must be in cargo metadata");
    let resolve = metadata
        .resolve
        .as_ref()
        .expect("cargo metadata must include the dependency graph");
    let node = resolve
        .nodes
        .iter()
        .find(|node| node.id == package.id)
        .expect("plotnik-tests must be in the resolved dependency graph");

    for &(dependency, key) in GRAMMARS {
        let dependency_id = &node
            .deps
            .iter()
            .find(|candidate| candidate.name == dependency)
            .unwrap_or_else(|| panic!("direct dependency `{dependency}` must resolve"))
            .pkg;
        let found = metadata
            .packages
            .iter()
            .find(|candidate| candidate.id == *dependency_id)
            .unwrap_or_else(|| panic!("package `{dependency_id}` must be in cargo metadata"));
        let root = found
            .manifest_path
            .parent()
            .unwrap_or_else(|| panic!("{dependency} package must have a parent directory"));
        let grammar_path = root.join("grammar/src/grammar.json");
        if !grammar_path.is_file() {
            panic!("{dependency} grammar.json must exist at {grammar_path}");
        }

        println!("cargo::rustc-env=PLOTNIK_TEST_GRAMMAR_{key}={grammar_path}");
        println!("cargo::rerun-if-changed={grammar_path}");
    }

    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=Cargo.toml");
    println!(
        "cargo::rerun-if-changed={}",
        manifest_dir.join("../../Cargo.lock").display()
    );
}
