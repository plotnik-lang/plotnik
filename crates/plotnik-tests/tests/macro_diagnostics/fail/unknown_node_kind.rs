// Bind-stage diagnostics (grammar mismatches) render the same way.

plotnik::query! {
    grammar = "arborium-javascript",
    "Q = (program (no_such_node_kind) @x)"
}

fn main() {}
