// Two expansions in one module: the fingerprint-named wrappers keep the
// generated internals (the `rt` alias, `mod matcher`) from colliding.

plotnik::query! {
    grammar = "arborium-javascript",
    "A = (program (expression_statement (identifier) @id))"
}

plotnik::query! {
    grammar = "arborium-javascript",
    "B = {(program)}"
}

fn main() {}
