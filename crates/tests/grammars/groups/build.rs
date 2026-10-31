fn main() {
    cc::Build::new()
        .std("c11")
        .include("src")
        .file("src/parser.c")
        .compile("tree-sitter-groups");
    println!("cargo::rerun-if-changed=src/parser.c");
    println!("cargo::rerun-if-changed=src/tree_sitter/parser.h");
}
