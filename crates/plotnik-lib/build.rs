use std::path::PathBuf;

// Locate each grammar `grammar.json` that ships inside an `arborium-*` dev-dependency
// and expose its path to the test suites via an env var. The fixture harness reads
// these to load the language a fixture selects (`==== input.<ext> ====`).
fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let manifest_path = PathBuf::from(&manifest_dir).join("Cargo.toml");

    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(&manifest_path)
        .exec()
        .expect("failed to run cargo metadata");

    for (package, env) in [
        ("arborium-javascript", "PLOTNIK_LIB_JAVASCRIPT_GRAMMAR_JSON"),
        ("arborium-typescript", "PLOTNIK_LIB_TYPESCRIPT_GRAMMAR_JSON"),
        ("arborium-dart", "PLOTNIK_LIB_DART_GRAMMAR_JSON"),
    ] {
        let found = metadata
            .packages
            .iter()
            .find(|p| p.name == package)
            .unwrap_or_else(|| panic!("{package} package not found"));
        let root = found
            .manifest_path
            .parent()
            .unwrap_or_else(|| panic!("{package} package has no parent dir"));
        let grammar_path = root.join("grammar/src/grammar.json");
        if !grammar_path.exists() {
            panic!("{package} grammar.json not found: {grammar_path}");
        }
        println!("cargo::rustc-env={env}={grammar_path}");
        println!("cargo::rerun-if-changed={grammar_path}");
    }

    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=Cargo.toml");
}
