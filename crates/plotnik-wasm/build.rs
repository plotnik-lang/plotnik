use std::path::{Path, PathBuf};

struct GrammarPackage {
    name: &'static str,
    feature: &'static str,
    dependency: &'static str,
    parser_dir: &'static str,
}

macro_rules! define_grammars {
    ($($name:ident => {
        feature: $feature:literal,
        dependency: $dependency:literal,
        parser_dir: $parser_dir:literal,
        symbol: $symbol:ident,
        aliases: [$($alias:literal),* $(,)?],
    }),* $(,)?) => {
        const GRAMMARS: &[GrammarPackage] = &[$(GrammarPackage {
            name: stringify!($name),
            feature: $feature,
            dependency: $dependency,
            parser_dir: $parser_dir,
        }),*];
    };
}

include!("grammars.rs");

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=grammars.rs");
    println!("cargo::rerun-if-changed=Cargo.toml");

    let enabled: Vec<_> = GRAMMARS
        .iter()
        .filter(|grammar| {
            let feature = grammar.feature.replace('-', "_").to_ascii_uppercase();
            std::env::var_os(format!("CARGO_FEATURE_{feature}")).is_some()
        })
        .collect();
    if enabled.is_empty() {
        return;
    }

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let manifest_path = PathBuf::from(manifest_dir).join("Cargo.toml");
    let features = enabled
        .iter()
        .map(|grammar| grammar.feature.to_owned())
        .collect();
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(&manifest_path)
        .features(cargo_metadata::CargoOpt::NoDefaultFeatures)
        .features(cargo_metadata::CargoOpt::SomeFeatures(features))
        .other_options(vec!["--locked".into()])
        .exec()
        .expect("failed to run cargo metadata");
    let package = metadata
        .packages
        .iter()
        .find(|package| package.manifest_path.as_std_path() == manifest_path)
        .expect("plotnik-wasm must be in cargo metadata");
    let resolve = metadata
        .resolve
        .as_ref()
        .expect("dependency graph must resolve");
    let node = resolve
        .nodes
        .iter()
        .find(|node| node.id == package.id)
        .expect("plotnik-wasm must be in the dependency graph");

    for grammar in enabled {
        let dependency = node
            .deps
            .iter()
            .find(|dependency| {
                dependency.name == grammar.dependency
                    && dependency
                        .dep_kinds
                        .iter()
                        .any(|kind| kind.kind == cargo_metadata::DependencyKind::Build)
            })
            .unwrap_or_else(|| {
                panic!(
                    "enabled grammar {} must be a direct build dependency",
                    grammar.name
                )
            });
        let package = metadata
            .packages
            .iter()
            .find(|package| package.id == dependency.pkg)
            .expect("grammar dependency must be in cargo metadata");
        let package_root = package
            .manifest_path
            .parent()
            .expect("package has no parent dir");
        let grammar_path = package_root.join(grammar.parser_dir).join("grammar.json");
        assert!(
            grammar_path.is_file(),
            "grammar.json not found: {grammar_path}"
        );

        println!(
            "cargo::rustc-env=PLOTNIK_WASM_GRAMMAR_JSON_{}={grammar_path}",
            grammar.name
        );
        println!(
            "cargo::rustc-env=PLOTNIK_WASM_GRAMMAR_SOURCE_{}={}@{}",
            grammar.name, package.name, package.version
        );
        // Scanners can include headers outside their parser directory.
        println!("cargo::rerun-if-changed={package_root}");
        compile_parser(package_root.as_std_path(), grammar);
    }
}

fn compile_parser(package_root: &Path, grammar: &GrammarPackage) {
    let parser_dir = package_root.join(grammar.parser_dir);
    let mut build = cc::Build::new();
    build
        .std("c11")
        .include(&parser_dir)
        .flag_if_supported("-Wno-unused-parameter")
        .file(parser_dir.join("parser.c"));

    if std::env::var("TARGET").expect("TARGET not set") == "wasm32-unknown-unknown" {
        let headers = std::env::var("DEP_TREE_SITTER_LANGUAGE_WASM_HEADERS")
            .expect("tree-sitter-language did not provide its WASM headers");
        build.include(headers);
    }

    let scanner = parser_dir.join("scanner.c");
    if scanner.exists() {
        build.file(scanner);
    }

    build.compile(grammar.dependency);
}
