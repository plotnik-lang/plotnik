use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::super::prepared::{ExtractedLexicalGrammar, LexicalGrammar, LexicalVariable};

pub type ExpandTokensResult<T> = Result<T, ExpandTokensError>;

#[derive(Debug, Error, Serialize, Deserialize)]
pub enum ExpandTokensError {
    #[error(
        "The rule `{0}` matches the empty string.
Tree-sitter does not support syntactic rules that match the empty string
unless they are used only as the grammar's start rule.
"
    )]
    EmptyString(String),
}

pub fn expand_tokens(grammar: ExtractedLexicalGrammar) -> ExpandTokensResult<LexicalGrammar> {
    let mut variables = Vec::with_capacity(grammar.variables.len());
    for variable in grammar.variables {
        if variable.rule.is_empty() {
            return Err(ExpandTokensError::EmptyString(variable.name));
        }
        variables.push(LexicalVariable {
            name: variable.name,
            kind: variable.kind,
        });
    }

    Ok(LexicalGrammar { variables })
}
