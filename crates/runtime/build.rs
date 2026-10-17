use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=src/cursor/checked.c");
    if std::env::var_os("CARGO_FEATURE_EXECUTION").is_none() {
        return;
    }

    let include = PathBuf::from(
        std::env::var_os("DEP_TREE_SITTER_INCLUDE")
            .expect("tree-sitter exports its include directory"),
    );
    let source = include.parent().expect("include has a parent").join("src");
    assert!(
        source.join("tree_cursor.h").is_file(),
        "tree-sitter 0.27.0 private cursor headers are required"
    );
    let mut build = cc::Build::new();
    if std::env::var("TARGET")
        .expect("cargo sets TARGET")
        .starts_with("wasm32-unknown")
    {
        build
            .include(
                std::env::var_os("DEP_TREE_SITTER_LANGUAGE_WASM_HEADERS")
                    .expect("tree-sitter-language exports its wasm C headers"),
            )
            .define("TREE_SITTER_WASM_STDLIB", "");
    }
    build
        .include(include)
        .include(source)
        .file("src/cursor/checked.c")
        .flag_if_supported("-std=c11")
        .compile("plotnik_checked_cursor");
}
