use std::sync::{Arc, LazyLock};

use crate::{Lang, LangInner};

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
        // Generate NodeTypes statics via proc macro
        $(
            #[cfg(feature = $feature)]
            plotnik_macros::generate_node_types!($node_types_key);
        )*

        // Generate static Lang definitions with LazyLock
        $(
            #[cfg(feature = $feature)]
            pub fn $fn_name() -> Lang {
                paste::paste! {
                    static LANG: LazyLock<Lang> = LazyLock::new(|| {
                        Arc::new(LangInner::new_static(
                            $name,
                            $ts_lang.into(),
                            &[<$node_types_key:upper _NODE_TYPES>],
                        ))
                    });
                }
                Arc::clone(&LANG)
            }
        )*

        pub fn from_name(s: &str) -> Option<Lang> {
            match s.to_ascii_lowercase().as_str() {
                $(
                    #[cfg(feature = $feature)]
                    $($alias)|* => Some($fn_name()),
                )*
                _ => None,
            }
        }

        pub fn from_ext(ext: &str) -> Option<Lang> {
            match ext.to_ascii_lowercase().as_str() {
                $(
                    #[cfg(feature = $feature)]
                    $($ext)|* => Some($fn_name()),
                )*
                _ => None,
            }
        }

        pub fn all() -> Vec<Lang> {
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
        feature: "lang-bash",
        name: "bash",
        ts_lang: arborium_bash::language(),
        node_types_key: "bash",
        names: ["bash", "sh", "shell"],
        extensions: ["sh", "bash", "zsh"],
    },
    c => {
        feature: "lang-c",
        name: "c",
        ts_lang: arborium_c::language(),
        node_types_key: "c",
        names: ["c"],
        extensions: ["c", "h"],
    },
    cpp => {
        feature: "lang-cpp",
        name: "cpp",
        ts_lang: arborium_cpp::language(),
        node_types_key: "cpp",
        names: ["cpp", "c++", "cxx", "cc"],
        extensions: ["cpp", "cc", "cxx", "hpp", "hh", "hxx", "h++", "c++"],
    },
    csharp => {
        feature: "lang-c-sharp",
        name: "c_sharp",
        ts_lang: arborium_c_sharp::language(),
        node_types_key: "c_sharp",
        names: ["csharp", "c#", "cs", "c_sharp"],
        extensions: ["cs"],
    },
    css => {
        feature: "lang-css",
        name: "css",
        ts_lang: arborium_css::language(),
        node_types_key: "css",
        names: ["css"],
        extensions: ["css"],
    },
    elixir => {
        feature: "lang-elixir",
        name: "elixir",
        ts_lang: arborium_elixir::language(),
        node_types_key: "elixir",
        names: ["elixir", "ex"],
        extensions: ["ex", "exs"],
    },
    go => {
        feature: "lang-go",
        name: "go",
        ts_lang: arborium_go::language(),
        node_types_key: "go",
        names: ["go", "golang"],
        extensions: ["go"],
    },
    haskell => {
        feature: "lang-haskell",
        name: "haskell",
        ts_lang: arborium_haskell::language(),
        node_types_key: "haskell",
        names: ["haskell", "hs"],
        extensions: ["hs", "lhs"],
    },
    hcl => {
        feature: "lang-hcl",
        name: "hcl",
        ts_lang: arborium_hcl::language(),
        node_types_key: "hcl",
        names: ["hcl", "terraform", "tf"],
        extensions: ["hcl", "tf", "tfvars"],
    },
    html => {
        feature: "lang-html",
        name: "html",
        ts_lang: arborium_html::language(),
        node_types_key: "html",
        names: ["html", "htm"],
        extensions: ["html", "htm"],
    },
    java => {
        feature: "lang-java",
        name: "java",
        ts_lang: arborium_java::language(),
        node_types_key: "java",
        names: ["java"],
        extensions: ["java"],
    },
    javascript => {
        feature: "lang-javascript",
        name: "javascript",
        ts_lang: arborium_javascript::language(),
        node_types_key: "javascript",
        names: ["javascript", "js", "jsx", "ecmascript", "es"],
        extensions: ["js", "mjs", "cjs", "jsx"],
    },
    json => {
        feature: "lang-json",
        name: "json",
        ts_lang: arborium_json::language(),
        node_types_key: "json",
        names: ["json"],
        extensions: ["json"],
    },
    kotlin => {
        feature: "lang-kotlin",
        name: "kotlin",
        ts_lang: arborium_kotlin::language(),
        node_types_key: "kotlin",
        names: ["kotlin", "kt"],
        extensions: ["kt", "kts"],
    },
    lua => {
        feature: "lang-lua",
        name: "lua",
        ts_lang: arborium_lua::language(),
        node_types_key: "lua",
        names: ["lua"],
        extensions: ["lua"],
    },
    nix => {
        feature: "lang-nix",
        name: "nix",
        ts_lang: arborium_nix::language(),
        node_types_key: "nix",
        names: ["nix"],
        extensions: ["nix"],
    },
    php => {
        feature: "lang-php",
        name: "php",
        ts_lang: arborium_php::language(),
        node_types_key: "php",
        names: ["php"],
        extensions: ["php"],
    },
    python => {
        feature: "lang-python",
        name: "python",
        ts_lang: arborium_python::language(),
        node_types_key: "python",
        names: ["python", "py"],
        extensions: ["py", "pyi", "pyw"],
    },
    ruby => {
        feature: "lang-ruby",
        name: "ruby",
        ts_lang: arborium_ruby::language(),
        node_types_key: "ruby",
        names: ["ruby", "rb"],
        extensions: ["rb", "rake", "gemspec"],
    },
    rust => {
        feature: "lang-rust",
        name: "rust",
        ts_lang: arborium_rust::language(),
        node_types_key: "rust",
        names: ["rust", "rs"],
        extensions: ["rs"],
    },
    scala => {
        feature: "lang-scala",
        name: "scala",
        ts_lang: arborium_scala::language(),
        node_types_key: "scala",
        names: ["scala"],
        extensions: ["scala", "sc"],
    },
    swift => {
        feature: "lang-swift",
        name: "swift",
        ts_lang: arborium_swift::language(),
        node_types_key: "swift",
        names: ["swift"],
        extensions: ["swift"],
    },
    typescript => {
        feature: "lang-typescript",
        name: "typescript",
        ts_lang: arborium_typescript::language(),
        node_types_key: "typescript",
        names: ["typescript", "ts"],
        extensions: ["ts", "mts", "cts"],
    },
    tsx => {
        feature: "lang-tsx",
        name: "tsx",
        ts_lang: arborium_tsx::language(),
        node_types_key: "tsx",
        names: ["tsx"],
        extensions: ["tsx"],
    },
    yaml => {
        feature: "lang-yaml",
        name: "yaml",
        ts_lang: arborium_yaml::language(),
        node_types_key: "yaml",
        names: ["yaml", "yml"],
        extensions: ["yaml", "yml"],
    },
}
