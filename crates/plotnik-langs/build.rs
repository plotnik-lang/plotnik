use std::path::PathBuf;

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let manifest_path = PathBuf::from(&manifest_dir).join("Cargo.toml");

    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(&manifest_path)
        .exec()
        .expect("failed to run cargo metadata");

    for package in &metadata.packages {
        if !package.name.starts_with("tree-sitter-") {
            continue;
        }

        let Some(feature_name) = tree_sitter_package_to_feature(&package.name) else {
            continue;
        };

        if std::env::var(format!("CARGO_FEATURE_{}", feature_name.to_uppercase())).is_err() {
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
                feature_name.to_uppercase(),
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
        "tree-sitter-php" => vec![("", "php/src/node-types.json")],
        "tree-sitter-typescript" => vec![
            ("", "typescript/src/node-types.json"),
            ("_TSX", "tsx/src/node-types.json"),
        ],
        _ => vec![("", "src/node-types.json")],
    }
}

fn tree_sitter_package_to_feature(package_name: &str) -> Option<&str> {
    match package_name {
        "tree-sitter-bash" => Some("bash"),
        "tree-sitter-c" => Some("c"),
        "tree-sitter-cpp" => Some("cpp"),
        "tree-sitter-c-sharp" => Some("csharp"),
        "tree-sitter-css" => Some("css"),
        "tree-sitter-elixir" => Some("elixir"),
        "tree-sitter-go" => Some("go"),
        "tree-sitter-haskell" => Some("haskell"),
        "tree-sitter-hcl" => Some("hcl"),
        "tree-sitter-html" => Some("html"),
        "tree-sitter-java" => Some("java"),
        "tree-sitter-javascript" => Some("javascript"),
        "tree-sitter-json" => Some("json"),
        "tree-sitter-kotlin-sg" => Some("kotlin"),
        "tree-sitter-lua" => Some("lua"),
        "tree-sitter-nix" => Some("nix"),
        "tree-sitter-php" => Some("php"),
        "tree-sitter-python" => Some("python"),
        "tree-sitter-ruby" => Some("ruby"),
        "tree-sitter-rust" => Some("rust"),
        "tree-sitter-scala" => Some("scala"),
        "tree-sitter-solidity" => Some("solidity"),
        "tree-sitter-swift" => Some("swift"),
        "tree-sitter-typescript" => Some("typescript"),
        "tree-sitter-yaml" => Some("yaml"),
        _ => None,
    }
}
