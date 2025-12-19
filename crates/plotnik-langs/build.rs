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

    for (key, _) in std::env::vars() {
        if key.starts_with("CARGO_FEATURE_LANG_") {
            println!("cargo::rerun-if-env-changed={}", key);
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
        "lang-ssh-config" => "SSH_CONFIG".to_string(),
        _ => feature
            .strip_prefix("lang-")
            .unwrap_or(feature)
            .to_uppercase()
            .replace('-', "_"),
    }
}

fn arborium_package_to_feature(package_name: &str) -> Option<&str> {
    match package_name {
        "arborium-ada" => Some("lang-ada"),
        "arborium-agda" => Some("lang-agda"),
        "arborium-asciidoc" => Some("lang-asciidoc"),
        "arborium-asm" => Some("lang-asm"),
        "arborium-awk" => Some("lang-awk"),
        "arborium-bash" => Some("lang-bash"),
        "arborium-batch" => Some("lang-batch"),
        "arborium-c" => Some("lang-c"),
        "arborium-c-sharp" => Some("lang-c-sharp"),
        "arborium-caddy" => Some("lang-caddy"),
        "arborium-capnp" => Some("lang-capnp"),
        "arborium-clojure" => Some("lang-clojure"),
        "arborium-cmake" => Some("lang-cmake"),
        "arborium-commonlisp" => Some("lang-commonlisp"),
        "arborium-cpp" => Some("lang-cpp"),
        "arborium-css" => Some("lang-css"),
        "arborium-d" => Some("lang-d"),
        "arborium-dart" => Some("lang-dart"),
        "arborium-devicetree" => Some("lang-devicetree"),
        "arborium-diff" => Some("lang-diff"),
        "arborium-dockerfile" => Some("lang-dockerfile"),
        "arborium-dot" => Some("lang-dot"),
        "arborium-elisp" => Some("lang-elisp"),
        "arborium-elixir" => Some("lang-elixir"),
        "arborium-elm" => Some("lang-elm"),
        "arborium-erlang" => Some("lang-erlang"),
        "arborium-fish" => Some("lang-fish"),
        "arborium-fsharp" => Some("lang-fsharp"),
        "arborium-gleam" => Some("lang-gleam"),
        "arborium-glsl" => Some("lang-glsl"),
        "arborium-go" => Some("lang-go"),
        "arborium-graphql" => Some("lang-graphql"),
        "arborium-haskell" => Some("lang-haskell"),
        "arborium-hcl" => Some("lang-hcl"),
        "arborium-hlsl" => Some("lang-hlsl"),
        "arborium-html" => Some("lang-html"),
        "arborium-idris" => Some("lang-idris"),
        "arborium-ini" => Some("lang-ini"),
        "arborium-java" => Some("lang-java"),
        "arborium-javascript" => Some("lang-javascript"),
        "arborium-jinja2" => Some("lang-jinja2"),
        "arborium-jq" => Some("lang-jq"),
        "arborium-json" => Some("lang-json"),
        "arborium-julia" => Some("lang-julia"),
        "arborium-kdl" => Some("lang-kdl"),
        "arborium-kotlin" => Some("lang-kotlin"),
        "arborium-lean" => Some("lang-lean"),
        "arborium-lua" => Some("lang-lua"),
        "arborium-markdown" => Some("lang-markdown"),
        "arborium-matlab" => Some("lang-matlab"),
        "arborium-meson" => Some("lang-meson"),
        "arborium-nginx" => Some("lang-nginx"),
        "arborium-ninja" => Some("lang-ninja"),
        "arborium-nix" => Some("lang-nix"),
        "arborium-objc" => Some("lang-objc"),
        "arborium-ocaml" => Some("lang-ocaml"),
        "arborium-perl" => Some("lang-perl"),
        "arborium-php" => Some("lang-php"),
        "arborium-postscript" => Some("lang-postscript"),
        "arborium-powershell" => Some("lang-powershell"),
        "arborium-prolog" => Some("lang-prolog"),
        "arborium-python" => Some("lang-python"),
        "arborium-query" => Some("lang-query"),
        "arborium-r" => Some("lang-r"),
        "arborium-rescript" => Some("lang-rescript"),
        "arborium-ron" => Some("lang-ron"),
        "arborium-ruby" => Some("lang-ruby"),
        "arborium-rust" => Some("lang-rust"),
        "arborium-scala" => Some("lang-scala"),
        "arborium-scheme" => Some("lang-scheme"),
        "arborium-scss" => Some("lang-scss"),
        "arborium-sparql" => Some("lang-sparql"),
        "arborium-sql" => Some("lang-sql"),
        "arborium-ssh-config" => Some("lang-ssh-config"),
        "arborium-starlark" => Some("lang-starlark"),
        "arborium-svelte" => Some("lang-svelte"),
        "arborium-swift" => Some("lang-swift"),
        "arborium-textproto" => Some("lang-textproto"),
        "arborium-thrift" => Some("lang-thrift"),
        "arborium-tlaplus" => Some("lang-tlaplus"),
        "arborium-toml" => Some("lang-toml"),
        "arborium-tsx" => Some("lang-tsx"),
        "arborium-typescript" => Some("lang-typescript"),
        "arborium-typst" => Some("lang-typst"),
        "arborium-uiua" => Some("lang-uiua"),
        "arborium-vb" => Some("lang-vb"),
        "arborium-verilog" => Some("lang-verilog"),
        "arborium-vhdl" => Some("lang-vhdl"),
        "arborium-vim" => Some("lang-vim"),
        "arborium-vue" => Some("lang-vue"),
        "arborium-x86asm" => Some("lang-x86asm"),
        "arborium-xml" => Some("lang-xml"),
        "arborium-yaml" => Some("lang-yaml"),
        "arborium-yuri" => Some("lang-yuri"),
        "arborium-zig" => Some("lang-zig"),
        "arborium-zsh" => Some("lang-zsh"),
        _ => None,
    }
}
