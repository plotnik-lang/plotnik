use std::fmt;

use super::rules::{Alias, Rule, Symbol};

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

impl Variable {
    pub(in crate::core::grammar) fn anonymous(name: String, rule: Rule) -> Self {
        Self {
            name,
            kind: VariableType::Anonymous,
            rule,
        }
    }

    pub(in crate::core::grammar) fn auxiliary(name: String, rule: Rule) -> Self {
        Self {
            name,
            kind: VariableType::Auxiliary,
            rule,
        }
    }

    pub(in crate::core::grammar) fn named(name: String, rule: Rule) -> Self {
        Self {
            name,
            kind: VariableType::Named,
            rule,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PrecedenceEntry {
    Name(String),
    Symbol(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReservedWordSet<T> {
    pub name: String,
    pub reserved_words: Vec<T>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalToken {
    pub name: String,
    pub kind: VariableType,
    pub corresponding_internal_token: Option<Symbol>,
}

impl ExternalToken {
    pub(in crate::core::grammar) fn external(name: String, kind: VariableType) -> Self {
        Self {
            name,
            kind,
            corresponding_internal_token: None,
        }
    }

    pub(in crate::core::grammar) fn internal(
        name: String,
        kind: VariableType,
        symbol: Symbol,
    ) -> Self {
        Self {
            name,
            kind,
            corresponding_internal_token: Some(symbol),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct InternedGrammar {
    pub variables: Vec<Variable>,
    pub extra_symbols: Vec<Rule>,
    pub external_tokens: Vec<Variable>,
    pub variables_to_inline: Vec<Symbol>,
    pub supertype_symbols: Vec<Symbol>,
    pub word_token: Option<Symbol>,
    pub reserved_word_sets: Vec<ReservedWordSet<Rule>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractedSyntaxGrammar {
    pub variables: Vec<Variable>,
    pub extra_symbols: Vec<Symbol>,
    pub external_tokens: Vec<ExternalToken>,
    pub variables_to_inline: Vec<Symbol>,
    pub supertype_symbols: Vec<Symbol>,
    pub word_token: Option<Symbol>,
    pub reserved_word_sets: Vec<ReservedWordSet<Symbol>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractedLexicalGrammar {
    pub variables: Vec<Variable>,
    pub separators: Vec<Rule>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct LexicalVariable {
    pub name: String,
    pub kind: VariableType,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct LexicalGrammar {
    pub variables: Vec<LexicalVariable>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProductionStep {
    pub symbol: Symbol,
    pub alias: Option<Alias>,
    pub field_name: Option<String>,
}

impl ProductionStep {
    pub(in crate::core::grammar) fn inherit_inline_metadata_from(&mut self, parent: &Self) {
        if let Some(alias) = &parent.alias {
            self.alias = Some(alias.clone());
        }
        if let Some(field_name) = &parent.field_name {
            self.field_name = Some(field_name.clone());
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Production {
    pub steps: Vec<ProductionStep>,
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

#[derive(Debug, Default)]
pub struct SyntaxGrammar {
    pub variables: Vec<SyntaxVariable>,
    pub extra_symbols: Vec<Symbol>,
    pub external_tokens: Vec<ExternalToken>,
    pub variables_to_inline: Vec<Symbol>,
    pub supertype_symbols: Vec<Symbol>,
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
