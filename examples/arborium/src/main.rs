use arborium_tree_sitter::{Language, Parser};

#[allow(dead_code, unused_imports)]
mod generated;

plotnik::query! {
    "Q = (program (expression_statement (identifier) @id))",
    grammar = "arborium-javascript",
}

fn main() {
    let language: Language = arborium_javascript::language().into();
    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .expect("Arborium JavaScript language loads");

    let source = "answer;";
    let tree = parser.parse(source, None).expect("source parses");
    let result = Q::parse(&tree, source)
        .expect("runtime limits fit")
        .expect("query matches");
    let standalone = generated::Standalone::parse(&tree, source)
        .expect("runtime limits fit")
        .expect("generated query matches");
    let _: arborium_tree_sitter::Node<'_> = result.id;
    let _: plotnik_rt::Node<'_> = result.id;
    assert_eq!(standalone.id, result.id);
    assert_eq!(result.id.utf8_text(source.as_bytes()), Ok("answer"));
}
