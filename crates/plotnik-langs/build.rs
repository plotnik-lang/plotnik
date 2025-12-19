use std::path::PathBuf;

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let manifest_path = PathBuf::from(&manifest_dir).join("Cargo.toml");

    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(&manifest_path)
        .exec()
        .expect("failed to run cargo metadata");

    for package in &metadata.packages {
        if !package.name.starts_with("arborium-") {
            continue;
        }

        let Some(feature_name) = arborium_package_to_feature(&package.name) else {
            continue;
        };

        let env_feature = feature_name.to_uppercase().replace('-', "_");
        if std::env::var(format!("CARGO_FEATURE_{}", env_feature)).is_err() {
            continue;
        }

        let package_root = package
            .manifest_path
            .parent()
            .expect("package has no parent dir");

        let node_types_paths = get_node_types_paths(&package.name);
        for (suffix, rel_path) in node_types_paths {
            let node_types_path = package_root.join(rel_path);

            if !node_types_path.exists() {
                panic!(
                    "node-types.json not found for {}: {}",
                    package.name, node_types_path
                );
            }

            let env_var_name = format!(
                "PLOTNIK_NODE_TYPES_{}{}",
                feature_to_node_types_key(feature_name),
                suffix
            );
            println!("cargo::rustc-env={}={}", env_var_name, node_types_path);
            println!("cargo::rerun-if-changed={}", node_types_path);
        }
    }

    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=Cargo.toml");
}

fn get_node_types_paths(package_name: &str) -> Vec<(&'static str, &'static str)> {
    match package_name {
        // All arborium crates use consistent path
        _ => vec![("", "grammar/src/node-types.json")],
    }
}

fn feature_to_node_types_key(feature: &str) -> String {
    match feature {
        "lang-c-sharp" => "C_SHARP".to_string(),
        _ => feature
            .strip_prefix("lang-")
            .unwrap_or(feature)
            .to_uppercase()
            .replace('-', "_"),
    }
}

fn arborium_package_to_feature(package_name: &str) -> Option<&str> {
    match package_name {
        "arborium-bash" => Some("lang-bash"),
        "arborium-c" => Some("lang-c"),
        "arborium-cpp" => Some("lang-cpp"),
        "arborium-c-sharp" => Some("lang-c-sharp"),
        "arborium-css" => Some("lang-css"),
        "arborium-elixir" => Some("lang-elixir"),
        "arborium-go" => Some("lang-go"),
        "arborium-haskell" => Some("lang-haskell"),
        "arborium-hcl" => Some("lang-hcl"),
        "arborium-html" => Some("lang-html"),
        "arborium-java" => Some("lang-java"),
        "arborium-javascript" => Some("lang-javascript"),
        "arborium-json" => Some("lang-json"),
        "arborium-kotlin" => Some("lang-kotlin"),
        "arborium-lua" => Some("lang-lua"),
        "arborium-nix" => Some("lang-nix"),
        "arborium-php" => Some("lang-php"),
        "arborium-python" => Some("lang-python"),
        "arborium-ruby" => Some("lang-ruby"),
        "arborium-rust" => Some("lang-rust"),
        "arborium-scala" => Some("lang-scala"),
        "arborium-swift" => Some("lang-swift"),
        "arborium-typescript" => Some("lang-typescript"),
        "arborium-tsx" => Some("lang-tsx"),
        "arborium-yaml" => Some("lang-yaml"),
        _ => None,
    }
}
