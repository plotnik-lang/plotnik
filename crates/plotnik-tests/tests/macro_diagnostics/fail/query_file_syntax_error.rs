// A syntax error inside a `file =` query must render the compiler's
// annotate-snippets diagnostic, the same as the inline form — pinning that the
// file-backed path doesn't degrade the UX to a bare span on `file = "..."`.

plotnik::query! {
    grammar = "arborium-javascript",
    file = "broken_query.ptk",
}

fn main() {}
