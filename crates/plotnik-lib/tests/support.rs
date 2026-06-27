use std::fs;
use std::path::PathBuf;
use std::sync::LazyLock;

pub fn load_arborium_grammar_json(package: &str) -> String {
    let found = arborium_metadata()
        .packages
        .iter()
        .find(|p| p.name == package)
        .unwrap_or_else(|| panic!("{package} package not found"));
    let root = found
        .manifest_path
        .parent()
        .unwrap_or_else(|| panic!("{package} package has no parent dir"));
    let grammar_path = root.join("grammar/src/grammar.json");
    fs::read_to_string(&grammar_path)
        .unwrap_or_else(|e| panic!("{package} grammar.json not found at {grammar_path}: {e}"))
}

fn arborium_metadata() -> &'static cargo_metadata::Metadata {
    static METADATA: LazyLock<cargo_metadata::Metadata> = LazyLock::new(|| {
        let manifest_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        cargo_metadata::MetadataCommand::new()
            .manifest_path(manifest_path)
            .exec()
            .expect("cargo metadata should resolve dev-dependencies")
    });
    &METADATA
}
