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

    // Check for features not defined in builtin.rs
    check_lang_definitions(&enabled_features, &out_dir, &manifest_dir);

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

        // Process grammar.json
        let grammar_path = package_root.join("grammar/src/grammar.json");
        if !grammar_path.exists() {
            panic!(
                "grammar.json not found for {}: {}",
                package.name, grammar_path
            );
        }
        process_json_file(
            &grammar_path,
            &out_dir,
            &lang_key,
            "GRAMMAR",
            "grammar",
            |json| {
                plotnik_core::grammar::Grammar::from_json(json)
                    .expect("failed to parse grammar.json")
            },
        );

        // Process node-types.json
        let node_types_path = package_root.join("grammar/src/node-types.json");
        if node_types_path.exists() {
            process_json_file(
                &node_types_path,
                &out_dir,
                &lang_key,
                "NODE_TYPES",
                "node_types",
                |json| {
                    serde_json::from_str::<Vec<plotnik_core::RawNode>>(json)
                        .expect("failed to parse node-types.json")
                },
            );
        } else {
            panic!(
                "node-types.json not found for {}: {}",
                package.name, node_types_path
            );
        }
    }

    for (key, _) in std::env::vars() {
        if key.starts_with("CARGO_FEATURE_LANG_") {
            println!("cargo::rerun-if-env-changed={}", key);
        }
    }

    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=Cargo.toml");
}

/// Parse JSON, serialize to binary, write to OUT_DIR, and set env var.
fn process_json_file<T, P, F>(
    json_path: P,
    out_dir: &Path,
    lang_key: &str,
    env_prefix: &str,
    file_suffix: &str,
    parse: F,
) where
    T: Serialize,
    P: AsRef<Path>,
    F: FnOnce(&str) -> T,
{
    let json_path = json_path.as_ref();
    let json = std::fs::read_to_string(json_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", json_path.display(), e));

    let parsed = parse(&json);

    let binary = postcard::to_allocvec(&parsed).expect("serialization should not fail");
    let binary_path = out_dir.join(format!("{}.{}", lang_key.to_lowercase(), file_suffix));
    std::fs::write(&binary_path, &binary)
        .unwrap_or_else(|e| panic!("failed to write {}: {}", binary_path.display(), e));

    println!(
        "cargo::rustc-env=PLOTNIK_{}_{}={}",
        env_prefix,
        lang_key,
        binary_path.display()
    );
    println!("cargo::rerun-if-changed={}", json_path.display());
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

fn check_lang_definitions(enabled_features: &[String], out_dir: &Path, manifest_dir: &str) {
    // Parse builtin.rs to find defined languages
    let builtin_path = PathBuf::from(manifest_dir).join("src/builtin.rs");
    let builtin_src = std::fs::read_to_string(&builtin_path)
        .expect("failed to read builtin.rs");

    // Extract feature: "lang-*" patterns from define_langs! macro
    let defined_langs: Vec<&str> = builtin_src
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("feature:") {
                // feature: "lang-foo",
                trimmed
                    .strip_prefix("feature:")
                    .and_then(|s| s.trim().strip_prefix('"'))
                    .and_then(|s| s.strip_suffix(',').or(Some(s)))
                    .and_then(|s| s.strip_suffix('"'))
            } else {
                None
            }
        })
        .collect();

    let mut errors = Vec::new();
    for feature in enabled_features {
        if !defined_langs.contains(&feature.as_str()) {
            errors.push(format!(
                "compile_error!(\"Feature `{feature}` enabled but not defined in builtin.rs. \
                Add language metadata to define_langs! macro.\");"
            ));
        }
    }

    let check_file = out_dir.join("lang_check.rs");
    if errors.is_empty() {
        std::fs::write(&check_file, "// All enabled features are defined in builtin.rs\n")
            .expect("failed to write lang_check.rs");
    } else {
        std::fs::write(&check_file, errors.join("\n"))
            .expect("failed to write lang_check.rs");
    }

    println!("cargo::rerun-if-changed={}", builtin_path.display());
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
