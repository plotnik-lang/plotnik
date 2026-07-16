use tree_sitter::{Parser, Tree};

#[derive(Clone, Copy, Debug)]
pub enum Language {
    JavaScript,
    TypeScript,
    Dart,
}

#[must_use]
pub fn parse(language: Language, source: &str) -> Tree {
    let language = match language {
        Language::JavaScript => arborium_javascript::language().into(),
        Language::TypeScript => arborium_typescript::language().into(),
        Language::Dart => arborium_dart::language().into(),
    };
    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .expect("snapshot language must be compatible with tree-sitter");
    parser
        .parse(source, None)
        .expect("tree-sitter must return a syntax tree")
}
