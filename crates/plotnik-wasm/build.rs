use std::fs;
use std::path::PathBuf;

use plotnik_lib::grammar::raw::RawGrammar;

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let manifest_path = PathBuf::from(&manifest_dir).join("Cargo.toml");

    let enabled_features: Vec<String> = std::env::vars()
        .filter_map(|(key, _)| {
            key.strip_prefix("CARGO_FEATURE_LANG_")
                .map(|suffix| format!("lang-{}", suffix.to_lowercase().replace('_', "-")))
        })
        .collect();

    if enabled_features.is_empty() {
        println!("cargo::rerun-if-changed=build.rs");
        println!("cargo::rerun-if-changed=Cargo.toml");
        return;
    }

    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(&manifest_path)
        .features(cargo_metadata::CargoOpt::SomeFeatures(enabled_features))
        .exec()
        .expect("failed to run cargo metadata");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR not set"));

    for package in &metadata.packages {
        let Some(feature_name) = arborium_package_to_feature(&package.name) else {
            continue;
        };

        let package_root = package
            .manifest_path
            .parent()
            .expect("package has no parent dir");
        let grammar_path = package_root.join("grammar/src/grammar.json");
        if !grammar_path.exists() {
            panic!(
                "grammar.json not found for {}: {}",
                package.name, grammar_path
            );
        }

        let json = fs::read_to_string(&grammar_path).unwrap_or_else(|error| {
            panic!("failed to read {grammar_path}: {error}");
        });
        let raw = RawGrammar::from_json(&json).unwrap_or_else(|error| {
            panic!("failed to parse {grammar_path}: {error}");
        });
        let compact_json = raw.to_json().unwrap_or_else(|error| {
            panic!("failed to serialize {grammar_path}: {error}");
        });

        let file_key = feature_to_file_key(&feature_name);
        let out_path = out_dir.join(format!("{file_key}.grammar.json"));
        fs::write(&out_path, compact_json).unwrap_or_else(|error| {
            panic!("failed to write {}: {error}", out_path.display());
        });

        let env_key = feature_to_env_key(&feature_name);
        println!(
            "cargo::rustc-env=PLOTNIK_WASM_GRAMMAR_JSON_{}={}",
            env_key,
            out_path.display()
        );
        println!("cargo::rerun-if-changed={grammar_path}");
    }

    for (key, _) in std::env::vars() {
        if key.starts_with("CARGO_FEATURE_LANG_") {
            println!("cargo::rerun-if-env-changed={}", key);
        }
    }

    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=Cargo.toml");
}

fn feature_to_file_key(feature: &str) -> String {
    feature.strip_prefix("lang-").unwrap_or(feature).to_string()
}

fn feature_to_env_key(feature: &str) -> String {
    feature
        .strip_prefix("lang-")
        .unwrap_or(feature)
        .to_uppercase()
        .replace('-', "_")
}

fn arborium_package_to_feature(package_name: &str) -> Option<String> {
    match package_name {
        "arborium-javascript" => Some("lang-javascript".to_string()),
        "arborium-typescript" => Some("lang-typescript".to_string()),
        _ => None,
    }
}
