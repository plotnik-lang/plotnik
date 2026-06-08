use std::fmt;

use super::{
    nfa::Nfa,
    rules::{Alias, Associativity, Precedence, Rule, Symbol},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum VariableType {
    Hidden,
    Auxiliary,
    Anonymous,
    Named,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Variable {
    pub name: String,
    pub kind: VariableType,
    pub rule: Rule,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PrecedenceEntry {
    Name(String),
    Symbol(String),
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ReservedWordContext<T> {
    pub name: String,
    pub reserved_words: Vec<T>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ResolvedGrammar {
    pub variables: Vec<Variable>,
    pub extra_symbols: Vec<Rule>,
    pub external_tokens: Vec<Variable>,
    pub variables_to_inline: Vec<Symbol>,
    pub supertype_symbols: Vec<Symbol>,
    pub word_token: Option<Symbol>,
    pub reserved_word_sets: Vec<ReservedWordContext<Rule>>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ExtractedSyntaxGrammar {
    pub variables: Vec<Variable>,
    pub extra_symbols: Vec<Symbol>,
    pub external_tokens: Vec<ExternalToken>,
    pub variables_to_inline: Vec<Symbol>,
    pub supertype_symbols: Vec<Symbol>,
    pub word_token: Option<Symbol>,
    pub reserved_word_sets: Vec<ReservedWordContext<Symbol>>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ExtractedLexicalGrammar {
    pub variables: Vec<Variable>,
    pub separators: Vec<Rule>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct LexicalVariable {
    pub name: String,
    pub kind: VariableType,
    pub implicit_precedence: i32,
    pub start_state: u32,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct LexicalGrammar {
    pub nfa: Nfa,
    pub variables: Vec<LexicalVariable>,
}

// Extracted syntax grammar

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProductionStep {
    pub symbol: Symbol,
    pub precedence: Precedence,
    pub associativity: Option<Associativity>,
    pub alias: Option<Alias>,
    pub field_name: Option<String>,
    pub reserved_word_set_id: ReservedWordSetId,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ReservedWordSetId(pub usize);

impl fmt::Display for ReservedWordSetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Production {
    pub steps: Vec<ProductionStep>,
    pub dynamic_precedence: i32,
}

#[derive(Default)]
pub struct InlinedProductionMap {
    pub productions: Vec<Production>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntaxVariable {
    pub name: String,
    pub kind: VariableType,
    pub productions: Vec<Production>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalToken {
    pub name: String,
    pub kind: VariableType,
    pub corresponding_internal_token: Option<Symbol>,
}

#[derive(Debug, Default)]
pub struct SyntaxGrammar {
    pub variables: Vec<SyntaxVariable>,
    pub extra_symbols: Vec<Symbol>,
    pub external_tokens: Vec<ExternalToken>,
    pub supertype_symbols: Vec<Symbol>,
    pub variables_to_inline: Vec<Symbol>,
    pub word_token: Option<Symbol>,
}

impl VariableType {
    #[must_use]
    pub fn is_visible(self) -> bool {
        self == Self::Named || self == Self::Anonymous
    }
}

impl fmt::Display for PrecedenceEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Name(n) => write!(f, "'{n}'"),
            Self::Symbol(s) => write!(f, "$.{s}"),
        }
    }
}
