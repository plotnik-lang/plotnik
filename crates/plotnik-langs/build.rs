use std::path::{Path, PathBuf};

use serde::Serialize;

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let manifest_path = PathBuf::from(&manifest_dir).join("Cargo.toml");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR not set"));

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

        let lang_key = feature_to_lang_key(&feature_name);

        let grammar_path = package_root.join("grammar/src/grammar.json");
        if !grammar_path.exists() {
            panic!(
                "grammar.json not found for {}: {}",
                package.name, grammar_path
            );
        }
        let grammar_json = std::fs::read_to_string(&grammar_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {}", grammar_path, e));
        let grammar = plotnik_core::grammar::Grammar::from_json(&grammar_json)
            .expect("failed to parse grammar.json");
        println!("cargo::rerun-if-changed={grammar_path}");

        write_binary(&out_dir, &lang_key, "GRAMMAR", "grammar", &grammar);

        write_binary(
            &out_dir,
            &lang_key,
            "NODE_SHAPES",
            "node_shapes",
            &grammar.node_shapes(),
        );
    }

    for (key, _) in std::env::vars() {
        if key.starts_with("CARGO_FEATURE_LANG_") {
            println!("cargo::rerun-if-env-changed={}", key);
        }
    }

    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=Cargo.toml");
}

/// Serialize to binary, write to OUT_DIR, and set env var.
fn write_binary<T>(out_dir: &Path, lang_key: &str, env_prefix: &str, file_suffix: &str, parsed: &T)
where
    T: Serialize,
{
    let binary = postcard::to_allocvec(parsed).expect("serialization should not fail");
    let binary_path = out_dir.join(format!("{}.{}", lang_key.to_lowercase(), file_suffix));
    std::fs::write(&binary_path, &binary)
        .unwrap_or_else(|e| panic!("failed to write {}: {}", binary_path.display(), e));

    println!(
        "cargo::rustc-env=PLOTNIK_{}_{}={}",
        env_prefix,
        lang_key,
        binary_path.display()
    );
}

fn feature_to_lang_key(feature: &str) -> String {
    match feature {
        "lang-c-sharp" => "C_SHARP".to_string(),
        "lang-ssh-config" => "SSH_CONFIG".to_string(),
        _ => feature
            .strip_prefix("lang-")
            .unwrap_or(feature)
            .to_uppercase()
            .replace('-', "_"),
    }
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
