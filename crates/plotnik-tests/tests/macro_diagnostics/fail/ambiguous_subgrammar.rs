// tree-sitter-typescript ships two grammars; without a selector the macro
// must list them instead of guessing.

plotnik::query! {
    grammar = "tree-sitter-typescript",
    "Q = (program)"
}

fn main() {}
