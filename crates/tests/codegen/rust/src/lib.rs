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
        Language::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        Language::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        Language::Dart => tree_sitter_dart::LANGUAGE.into(),
    };
    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .expect("snapshot language must be compatible with tree-sitter");
    parser
        .parse(source, None)
        .expect("tree-sitter must return a syntax tree")
}
