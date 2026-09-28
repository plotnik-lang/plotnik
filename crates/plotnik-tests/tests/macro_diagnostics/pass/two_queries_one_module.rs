// Two expansions in one module: the fingerprint-named wrappers keep the
// generated internals (the `rt` alias, `mod matcher`) from colliding.

plotnik::query! {
    "A = (program (expression_statement (identifier) @id))",
    grammar = "tree-sitter-javascript",
}

plotnik::query! {
    "B = {(program)}",
    grammar = "tree-sitter-javascript",
}

fn main() {}
