use std::sync::LazyLock;
use tree_sitter::Language;

#[derive(Debug, Clone)]
pub struct Lang {
    pub name: &'static str,
    pub ts_lang: Language,
    pub node_types_size: usize,
}

macro_rules! define_langs {
    (
        $(
            $fn_name:ident => {
                feature: $feature:literal,
                name: $name:literal,
                ts_lang: $ts_lang:expr,
                node_types_key: $node_types_key:literal,
                names: [$($alias:literal),* $(,)?],
                extensions: [$($ext:literal),* $(,)?] $(,)?
            }
        ),* $(,)?
    ) => {
        // Generate node_types_size constants via proc macro
        $(
            #[cfg(feature = $feature)]
            plotnik_macros::generate_node_types_size!($node_types_key);
        )*

        // Generate lazy accessor functions
        $(
            #[cfg(feature = $feature)]
            pub fn $fn_name() -> &'static Lang {
                paste::paste! {
                    static LANG: LazyLock<Lang> = LazyLock::new(|| Lang {
                        name: $name,
                        ts_lang: $ts_lang.into(),
                        node_types_size: [<$node_types_key:upper _NODE_TYPES_SIZE>],
                    });
                }
                &LANG
            }
        )*

        pub fn from_name(s: &str) -> Option<&'static Lang> {
            match s.to_ascii_lowercase().as_str() {
                $(
                    #[cfg(feature = $feature)]
                    $($alias)|* => Some($fn_name()),
                )*
                _ => None,
            }
        }

        pub fn from_ext(ext: &str) -> Option<&'static Lang> {
            match ext.to_ascii_lowercase().as_str() {
                $(
                    #[cfg(feature = $feature)]
                    $($ext)|* => Some($fn_name()),
                )*
                _ => None,
            }
        }

        pub fn all() -> Vec<&'static Lang> {
            vec![
                $(
                    #[cfg(feature = $feature)]
                    $fn_name(),
                )*
            ]
        }
    };
}

define_langs! {
    bash => {
        feature: "bash",
        name: "bash",
        ts_lang: tree_sitter_bash::LANGUAGE,
        node_types_key: "bash",
        names: ["bash", "sh", "shell"],
        extensions: ["sh", "bash", "zsh"],
    },
    c => {
        feature: "c",
        name: "c",
        ts_lang: tree_sitter_c::LANGUAGE,
        node_types_key: "c",
        names: ["c"],
        extensions: ["c", "h"],
    },
    cpp => {
        feature: "cpp",
        name: "cpp",
        ts_lang: tree_sitter_cpp::LANGUAGE,
        node_types_key: "cpp",
        names: ["cpp", "c++", "cxx", "cc"],
        extensions: ["cpp", "cc", "cxx", "hpp", "hh", "hxx", "h++", "c++"],
    },
    csharp => {
        feature: "csharp",
        name: "c_sharp",
        ts_lang: tree_sitter_c_sharp::LANGUAGE,
        node_types_key: "csharp",
        names: ["csharp", "c#", "cs", "c_sharp"],
        extensions: ["cs"],
    },
    css => {
        feature: "css",
        name: "css",
        ts_lang: tree_sitter_css::LANGUAGE,
        node_types_key: "css",
        names: ["css"],
        extensions: ["css"],
    },
    elixir => {
        feature: "elixir",
        name: "elixir",
        ts_lang: tree_sitter_elixir::LANGUAGE,
        node_types_key: "elixir",
        names: ["elixir", "ex"],
        extensions: ["ex", "exs"],
    },
    go => {
        feature: "go",
        name: "go",
        ts_lang: tree_sitter_go::LANGUAGE,
        node_types_key: "go",
        names: ["go", "golang"],
        extensions: ["go"],
    },
    haskell => {
        feature: "haskell",
        name: "haskell",
        ts_lang: tree_sitter_haskell::LANGUAGE,
        node_types_key: "haskell",
        names: ["haskell", "hs"],
        extensions: ["hs", "lhs"],
    },
    hcl => {
        feature: "hcl",
        name: "hcl",
        ts_lang: tree_sitter_hcl::LANGUAGE,
        node_types_key: "hcl",
        names: ["hcl", "terraform", "tf"],
        extensions: ["hcl", "tf", "tfvars"],
    },
    html => {
        feature: "html",
        name: "html",
        ts_lang: tree_sitter_html::LANGUAGE,
        node_types_key: "html",
        names: ["html", "htm"],
        extensions: ["html", "htm"],
    },
    java => {
        feature: "java",
        name: "java",
        ts_lang: tree_sitter_java::LANGUAGE,
        node_types_key: "java",
        names: ["java"],
        extensions: ["java"],
    },
    javascript => {
        feature: "javascript",
        name: "javascript",
        ts_lang: tree_sitter_javascript::LANGUAGE,
        node_types_key: "javascript",
        names: ["javascript", "js", "jsx", "ecmascript", "es"],
        extensions: ["js", "mjs", "cjs", "jsx"],
    },
    json => {
        feature: "json",
        name: "json",
        ts_lang: tree_sitter_json::LANGUAGE,
        node_types_key: "json",
        names: ["json"],
        extensions: ["json"],
    },
    kotlin => {
        feature: "kotlin",
        name: "kotlin",
        ts_lang: tree_sitter_kotlin::LANGUAGE,
        node_types_key: "kotlin",
        names: ["kotlin", "kt"],
        extensions: ["kt", "kts"],
    },
    lua => {
        feature: "lua",
        name: "lua",
        ts_lang: tree_sitter_lua::LANGUAGE,
        node_types_key: "lua",
        names: ["lua"],
        extensions: ["lua"],
    },
    nix => {
        feature: "nix",
        name: "nix",
        ts_lang: tree_sitter_nix::LANGUAGE,
        node_types_key: "nix",
        names: ["nix"],
        extensions: ["nix"],
    },
    php => {
        feature: "php",
        name: "php",
        ts_lang: tree_sitter_php::LANGUAGE_PHP,
        node_types_key: "php",
        names: ["php"],
        extensions: ["php"],
    },
    python => {
        feature: "python",
        name: "python",
        ts_lang: tree_sitter_python::LANGUAGE,
        node_types_key: "python",
        names: ["python", "py"],
        extensions: ["py", "pyi", "pyw"],
    },
    ruby => {
        feature: "ruby",
        name: "ruby",
        ts_lang: tree_sitter_ruby::LANGUAGE,
        node_types_key: "ruby",
        names: ["ruby", "rb"],
        extensions: ["rb", "rake", "gemspec"],
    },
    rust => {
        feature: "rust",
        name: "rust",
        ts_lang: tree_sitter_rust::LANGUAGE,
        node_types_key: "rust",
        names: ["rust", "rs"],
        extensions: ["rs"],
    },
    scala => {
        feature: "scala",
        name: "scala",
        ts_lang: tree_sitter_scala::LANGUAGE,
        node_types_key: "scala",
        names: ["scala"],
        extensions: ["scala", "sc"],
    },
    solidity => {
        feature: "solidity",
        name: "solidity",
        ts_lang: tree_sitter_solidity::LANGUAGE,
        node_types_key: "solidity",
        names: ["solidity", "sol"],
        extensions: ["sol"],
    },
    swift => {
        feature: "swift",
        name: "swift",
        ts_lang: tree_sitter_swift::LANGUAGE,
        node_types_key: "swift",
        names: ["swift"],
        extensions: ["swift"],
    },
    typescript => {
        feature: "typescript",
        name: "typescript",
        ts_lang: tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
        node_types_key: "typescript",
        names: ["typescript", "ts"],
        extensions: ["ts", "mts", "cts"],
    },
    tsx => {
        feature: "typescript",
        name: "tsx",
        ts_lang: tree_sitter_typescript::LANGUAGE_TSX,
        node_types_key: "typescript_tsx",
        names: ["tsx"],
        extensions: ["tsx"],
    },
    yaml => {
        feature: "yaml",
        name: "yaml",
        ts_lang: tree_sitter_yaml::LANGUAGE,
        node_types_key: "yaml",
        names: ["yaml", "yml"],
        extensions: ["yaml", "yml"],
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(feature = "javascript")]
    fn lang_from_name() {
        assert_eq!(from_name("js").unwrap().name, "javascript");
        assert_eq!(from_name("JavaScript").unwrap().name, "javascript");
        assert!(from_name("unknown").is_none());
    }

    #[test]
    #[cfg(feature = "go")]
    fn lang_from_name_golang() {
        assert_eq!(from_name("go").unwrap().name, "go");
        assert_eq!(from_name("golang").unwrap().name, "go");
        assert_eq!(from_name("GOLANG").unwrap().name, "go");
    }

    #[test]
    #[cfg(feature = "javascript")]
    fn lang_from_extension() {
        assert_eq!(from_ext("js").unwrap().name, "javascript");
        assert_eq!(from_ext("mjs").unwrap().name, "javascript");
    }

    #[test]
    #[cfg(feature = "typescript")]
    fn typescript_and_tsx() {
        assert_eq!(typescript().name, "typescript");
        assert_eq!(tsx().name, "tsx");
        assert_eq!(from_ext("ts").unwrap().name, "typescript");
        assert_eq!(from_ext("tsx").unwrap().name, "tsx");
    }

    #[test]
    #[cfg(feature = "javascript")]
    fn node_types_size_matches_runtime() {
        let runtime = std::fs::read_to_string(env!("PLOTNIK_NODE_TYPES_JAVASCRIPT"))
            .unwrap()
            .len();
        assert_eq!(javascript().node_types_size, runtime);
    }

    #[test]
    fn all_returns_enabled_langs() {
        let langs = all();
        assert!(!langs.is_empty());
        for lang in &langs {
            assert!(!lang.name.is_empty());
        }
    }
}
