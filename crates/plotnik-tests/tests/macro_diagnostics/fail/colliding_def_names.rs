// Distinct PascalCase definitions can collapse to one snake_case spelling;
// the macro refuses them instead of leaking a rustc duplicate-definition
// error from inside the expansion.

plotnik::query! {
    grammar = "arborium-javascript",
    r#"
    HTTPServer = (program)
    HttpServer = (program)
    "#
}

fn main() {}
