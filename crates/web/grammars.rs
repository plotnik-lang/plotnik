define_grammars! {
    javascript => {
        feature: "lang-javascript",
        dependency: "tree_sitter_javascript",
        parser_dir: "src",
        symbol: tree_sitter_javascript,
    },
    typescript => {
        feature: "lang-typescript",
        dependency: "tree_sitter_typescript",
        parser_dir: "typescript/src",
        symbol: tree_sitter_typescript,
    },
}
