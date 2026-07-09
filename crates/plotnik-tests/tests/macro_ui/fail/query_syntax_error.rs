// A query syntax error surfaces the compiler's annotate-snippets rendering
// inside compile_error!.

plotnik::query! {
    grammar = "arborium-javascript",
    r#"
    Q = (program
      (expression_statement (identifier) @id)
    "#
}

fn main() {}
