//! Language registry for the wasm bundle.
//!
//! A small sibling of the CLI's `language_registry`: one `define_langs!` row
//! per language, gated by a cargo feature. Runnable languages are fixed at
//! bundle build time (a tree-sitter `Tree` can't cross wasm instances — see
//! `docs/wip/playground-design.md` §6), so growing the set means growing
//! this table.
//!
//! Adding a language touches four places:
//! 1. `Cargo.toml`: a `lang-*` feature and its `arborium-*` dependency
//! 2. `build.rs`: `arborium_package_to_feature`
//! 3. a `define_langs!` row below
//! 4. the playground selector: `web/src/components/playground/Playground.tsx`

use plotnik_lib::grammar::Grammar;
use tree_sitter::{Language, Parser, Tree};

/// A language the bundle can run queries against: the tree-sitter parser
/// plus the Plotnik grammar metadata queries compile against.
pub struct Lang {
    ts_language: Language,
    grammar: Grammar,
}

impl Lang {
    pub fn grammar(&self) -> &Grammar {
        &self.grammar
    }

    pub fn parse_source(&self, source: &str) -> Tree {
        let mut parser = Parser::new();
        parser
            .set_language(&self.ts_language)
            .expect("failed to set language");
        parser.parse(source, None).expect("failed to parse source")
    }
}

/// build.rs embeds each enabled grammar's compacted `grammar.json`
/// (uncompressed — the served bundle is compressed as a whole).
#[cfg(any(feature = "lang-javascript", feature = "lang-typescript"))]
fn load_grammar(json: &str, name: &str) -> Grammar {
    use plotnik_lib::grammar::raw::RawGrammar;

    let raw = RawGrammar::from_json(json)
        .unwrap_or_else(|error| panic!("invalid embedded {name} grammar JSON: {error}"));
    Grammar::from_raw(&raw)
        .unwrap_or_else(|error| panic!("invalid embedded {name} grammar metadata: {error}"))
}

macro_rules! define_langs {
    (
        $(
            $fn_name:ident => {
                feature: $feature:literal,
                name: $name:literal,
                ts_lang: $ts_lang:expr,
                env_suffix: $env_suffix:literal,
                names: [$($alias:literal),* $(,)?] $(,)?
            }
        ),* $(,)?
    ) => {
        $(
            #[cfg(feature = $feature)]
            fn $fn_name() -> &'static Lang {
                static LANGUAGE: std::sync::LazyLock<Lang> = std::sync::LazyLock::new(|| Lang {
                    ts_language: $ts_lang.into(),
                    grammar: load_grammar(
                        include_str!(env!(concat!("PLOTNIK_WASM_GRAMMAR_JSON_", $env_suffix))),
                        $name,
                    ),
                });
                &LANGUAGE
            }
        )*

        fn from_name(input: &str) -> Option<&'static Lang> {
            match input.to_ascii_lowercase().as_str() {
                $(
                    #[cfg(feature = $feature)]
                    $($alias)|* => Some($fn_name()),
                )*
                _ => None,
            }
        }

        fn supported_names() -> &'static [&'static str] {
            &[
                $(
                    #[cfg(feature = $feature)]
                    $name,
                )*
            ]
        }
    };
}

define_langs! {
    javascript => {
        feature: "lang-javascript",
        name: "javascript",
        ts_lang: arborium_javascript::language(),
        env_suffix: "JAVASCRIPT",
        names: ["javascript", "js", "jsx", "ecmascript", "es"],
    },
    typescript => {
        feature: "lang-typescript",
        name: "typescript",
        ts_lang: arborium_typescript::language(),
        env_suffix: "TYPESCRIPT",
        names: ["typescript", "ts"],
    },
}

/// Resolve a user-supplied language name or alias. Unknown names are user
/// error, reported as a message — this is the outside boundary, never panic.
pub fn resolve(input: &str) -> Result<&'static Lang, String> {
    if let Some(lang) = from_name(input) {
        return Ok(lang);
    }
    let supported = supported_names();
    if supported.is_empty() {
        return Err("no languages are enabled in this plotnik-wasm build".to_string());
    }
    Err(format!(
        "unsupported language: {input}; supported languages: {}",
        supported.join(", ")
    ))
}
