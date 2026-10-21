use tree_sitter::{Language, Parser, Tree, WasmStore, wasmtime};

use crate::grammar::Grammar;

/// A Tree-sitter parser for a WASM grammar.
///
/// Pass metadata and WASM from the same grammar release. The constructor checks
/// every public node/field ID before queries can run against the resulting tree.
/// Trees retain their language and may outlive this parser.
pub struct WasmParser {
    grammar: Grammar,
    parser: Parser,
}

#[derive(Debug, thiserror::Error)]
pub enum WasmParserError {
    #[error("invalid WASM grammar name {0:?}: expected an ASCII identifier")]
    InvalidName(String),
    #[error("WASM grammar exceeds Tree-sitter's 32-bit artifact size limit")]
    TooLarge,
    #[error(transparent)]
    Wasm(#[from] tree_sitter::WasmError),
    #[error(transparent)]
    Language(#[from] tree_sitter::LanguageError),
    #[error("WASM/grammar metadata mismatch: {0}; use artifacts from the same release")]
    Metadata(String),
    #[error("Tree-sitter source parsing was cancelled or failed")]
    Parse,
}

impl WasmParser {
    pub fn new(grammar: Grammar, wasm: &[u8]) -> Result<Self, WasmParserError> {
        let name = grammar.name();
        if !valid_name(name) {
            return Err(WasmParserError::InvalidName(name.to_owned()));
        }
        if wasm.len() > u32::MAX as usize {
            return Err(WasmParserError::TooLarge);
        }

        let mut store = WasmStore::new(&wasmtime::Engine::default())?;
        let language = store.load_language(name, wasm)?;
        let mut parser = Parser::new();
        parser.set_wasm_store(store)?;
        parser.set_language(&language)?;
        validate_metadata(&grammar, &language)?;
        Ok(Self { grammar, parser })
    }

    pub fn grammar(&self) -> &Grammar {
        &self.grammar
    }

    /// Tree-sitter ABI version declared by the loaded WASM language.
    pub fn abi_version(&self) -> usize {
        self.parser
            .language()
            .expect("constructor set the parser language")
            .abi_version()
    }

    /// Parse source with Tree-sitter's usual syntax-error recovery. A tree with
    /// ERROR nodes is still a valid result and can be queried or inspected.
    pub fn parse(&mut self, source: &str) -> Result<Tree, WasmParserError> {
        self.parser
            .parse(source, None)
            .ok_or(WasmParserError::Parse)
    }
}

fn valid_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
        && bytes.all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

fn validate_metadata(grammar: &Grammar, language: &Language) -> Result<(), WasmParserError> {
    for (names, named) in [
        (grammar.all_named_node_kinds(), true),
        (grammar.all_anonymous_node_kinds(), false),
    ] {
        for name in names {
            let id = if named {
                grammar.resolve_named_node(name)
            } else {
                grammar.resolve_anonymous_node(name)
            }
            .expect("enumerated kind must resolve");
            let supertype = grammar.is_supertype(id);
            let id = u16::from(id);
            if language.node_kind_for_id(id) != Some(name)
                || language.node_kind_is_named(id) != (named && !supertype)
                || language.node_kind_is_supertype(id) != supertype
                || (!supertype && language.id_for_node_kind(name, named) != id)
            {
                return Err(WasmParserError::Metadata(format!(
                    "node {name:?} at ID {id}"
                )));
            }
        }
    }

    if language.field_count() != grammar.all_field_names().len() {
        return Err(WasmParserError::Metadata("field count differs".into()));
    }
    for name in grammar.all_field_names() {
        let id = u16::from(
            grammar
                .resolve_field(name)
                .expect("enumerated field must resolve"),
        );
        if language.field_name_for_id(id) != Some(name)
            || language.field_id_for_name(name).map(|id| id.get()) != Some(id)
        {
            return Err(WasmParserError::Metadata(format!(
                "field {name:?} at ID {id}"
            )));
        }
    }
    Ok(())
}
