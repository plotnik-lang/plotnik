use std::path::PathBuf;

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let manifest_path = PathBuf::from(&manifest_dir).join("Cargo.toml");

    // Collect enabled lang-* features from environment
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

        let node_types_path = package_root.join("grammar/src/node-types.json");
        if !node_types_path.exists() {
            panic!(
                "node-types.json not found for {}: {}",
                package.name, node_types_path
            );
        }

        let env_var_name = format!(
            "PLOTNIK_NODE_TYPES_{}",
            feature_to_node_types_key(&feature_name),
        );
        println!("cargo::rustc-env={}={}", env_var_name, node_types_path);
        println!("cargo::rerun-if-changed={}", node_types_path);
    }

    for (key, _) in std::env::vars() {
        if key.starts_with("CARGO_FEATURE_LANG_") {
            println!("cargo::rerun-if-env-changed={}", key);
        }
    }

    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=Cargo.toml");
}

fn feature_to_node_types_key(feature: &str) -> String {
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
