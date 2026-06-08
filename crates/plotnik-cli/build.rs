use std::fs;
use std::path::PathBuf;

use plotnik_core::grammar::raw::RawGrammar;

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
        if !package.name.starts_with("arborium-") {
            continue;
        }

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
            panic!("failed to read {}: {error}", grammar_path);
        });
        let raw = RawGrammar::from_json(&json).unwrap_or_else(|error| {
            panic!("failed to parse {}: {error}", grammar_path);
        });
        let bytes = raw.to_postcard().unwrap_or_else(|error| {
            panic!("failed to encode {}: {error}", grammar_path);
        });

        let file_key = feature_to_file_key(&feature_name);
        let out_path = out_dir.join(format!("{file_key}.grammar.bin"));
        fs::write(&out_path, bytes).unwrap_or_else(|error| {
            panic!("failed to write {}: {error}", out_path.display());
        });

        let env_key = feature_to_env_key(&feature_name);
        println!(
            "cargo::rustc-env=PLOTNIK_GRAMMAR_BIN_{}={}",
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
    const NON_LANGUAGE_PACKAGES: &[&str] = &[
        "arborium-docsrs-demo",
        "arborium-highlight",
        "arborium-host",
        "arborium-mdbook",
        "arborium-plugin-runtime",
        "arborium-rustdoc",
        "arborium-sysroot",
        "arborium-test-harness",
        "arborium-theme",
        "arborium-tree-sitter",
        "arborium-wire",
    ];

    if NON_LANGUAGE_PACKAGES.contains(&package_name) {
        return None;
    }

    package_name
        .strip_prefix("arborium-")
        .map(|suffix| format!("lang-{suffix}"))
}
