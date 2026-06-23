use std::path::PathBuf;

// Locate the JavaScript `grammar.json` that ships inside the `arborium-javascript`
// dev-dependency and expose its path to the test suites via an env var.
fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let manifest_path = PathBuf::from(&manifest_dir).join("Cargo.toml");

    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(&manifest_path)
        .exec()
        .expect("failed to run cargo metadata");

    let package = metadata
        .packages
        .iter()
        .find(|package| package.name == "arborium-javascript")
        .expect("arborium-javascript package not found");
    let package_root = package
        .manifest_path
        .parent()
        .expect("arborium-javascript package has no parent dir");
    let grammar_path = package_root.join("grammar/src/grammar.json");
    if !grammar_path.exists() {
        panic!("javascript grammar.json not found: {grammar_path}");
    }

    println!("cargo::rustc-env=PLOTNIK_LIB_JAVASCRIPT_GRAMMAR_JSON={grammar_path}");
    println!("cargo::rerun-if-changed={grammar_path}");
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=Cargo.toml");
}
