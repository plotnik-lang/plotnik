// The `crate` path is spliced into several nested modules; only an absolute
// path resolves the same way in all of them.

plotnik::query! {
    grammar = "arborium-javascript",
    crate = plotnik::rt,
    "Q = (program)"
}

fn main() {}
