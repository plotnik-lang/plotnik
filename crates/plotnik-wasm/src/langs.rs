//! Tree-sitter languages compiled into the WASM bundle.
//!
//! `grammars.rs` supplies both the build inputs and the runtime registry.

use plotnik_lib::GrammarIdentity;
use plotnik_lib::grammar::Grammar;
use tree_sitter::{Language, Parser, Tree};

/// A parser and the grammar metadata used to compile queries against it.
pub struct Lang {
    // WASM language handles are thread-bound. Cache metadata, then create a handle per parser.
    language: fn() -> Language,
    grammar: Grammar,
}

impl Lang {
    pub fn grammar(&self) -> &Grammar {
        &self.grammar
    }

    pub fn identity(&self) -> &GrammarIdentity {
        self.grammar
            .identity()
            .expect("embedded grammar has an identity")
    }

    pub fn parse_source(&self, source: &str) -> Tree {
        let mut parser = Parser::new();
        let language = (self.language)();
        parser
            .set_language(&language)
            .expect("failed to set language");
        parser.parse(source, None).expect("failed to parse source")
    }
}

macro_rules! define_grammars {
    ($($name:ident => {
        feature: $feature:literal,
        dependency: $dependency:literal,
        parser_dir: $parser_dir:literal,
        symbol: $symbol:ident,
        aliases: [$($alias:literal),* $(,)?],
    }),* $(,)?) => {
        #[cfg(any($(feature = $feature),*))]
        fn load_grammar(json: &str, source: &str) -> Grammar {
            use plotnik_lib::grammar::raw::RawGrammar;

            let raw = RawGrammar::from_json(json)
                .unwrap_or_else(|error| panic!("invalid embedded {source} grammar JSON: {error}"));
            let identity = GrammarIdentity::from_json_bytes(&raw.name, json.as_bytes(), source);
            Grammar::from_raw(&raw)
                .unwrap_or_else(|error| panic!("invalid embedded {source} grammar metadata: {error}"))
                .with_identity(identity)
        }

        $(
            #[cfg(feature = $feature)]
            fn $name() -> &'static Lang {
                unsafe extern "C" {
                    fn $symbol() -> *const ();
                }

                static LANGUAGE: std::sync::LazyLock<Lang> = std::sync::LazyLock::new(|| Lang {
                    // The build script compiles this entry point from the same package as the JSON.
                    language: || unsafe { tree_sitter_language::LanguageFn::from_raw($symbol) }.into(),
                    grammar: load_grammar(
                        include_str!(env!(concat!("PLOTNIK_WASM_GRAMMAR_JSON_", stringify!($name)))),
                        env!(concat!("PLOTNIK_WASM_GRAMMAR_SOURCE_", stringify!($name))),
                    ),
                });
                &LANGUAGE
            }
        )*

        fn from_name(input: &str) -> Option<&'static Lang> {
            match input.to_ascii_lowercase().as_str() {
                $(
                    #[cfg(feature = $feature)]
                    stringify!($name) $(| $alias)* => Some($name()),
                )*
                _ => None,
            }
        }

        fn supported_names() -> &'static [&'static str] {
            &[
                $(
                    #[cfg(feature = $feature)]
                    stringify!($name),
                )*
            ]
        }
    };
}

include!("../grammars.rs");

/// Resolve a user-supplied language name or alias.
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
