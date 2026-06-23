//! Raw tree-sitter grammar source-format types.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use super::GrammarError;

/// Direct parsed representation of tree-sitter `grammar.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawGrammar {
    /// Grammar name (e.g., "javascript", "rust").
    pub name: String,
    /// Production rules, preserving definition order.
    pub rules: IndexMap<String, RawRule>,
    /// Extra/trivia nodes (comments, whitespace).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extras: Vec<RawRule>,
    /// Precedence orderings.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub precedences: Vec<Vec<RawPrecedenceEntry>>,
    /// Expected conflicts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<Vec<String>>,
    /// External scanner tokens.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub externals: Vec<RawRule>,
    /// Rules to inline (hidden).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inline: Vec<String>,
    /// Supertype rules.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supertypes: Vec<String>,
    /// Keyword identifier rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub word: Option<String>,
    /// Reserved word contexts.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub reserved: IndexMap<String, Vec<RawRule>>,
    /// Parent grammar name (for inheritance).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inherits: Option<String>,
}

impl RawGrammar {
    /// Parse a raw grammar from a tree-sitter `grammar.json` string.
    pub fn from_json(json: &str) -> Result<Self, GrammarError> {
        serde_json::from_str(json).map_err(GrammarError::Json)
    }

    /// Serialize this raw grammar to compact JSON.
    pub fn to_json(&self) -> Result<String, GrammarError> {
        serde_json::to_string(self).map_err(GrammarError::Json)
    }
}

/// Raw rule matching tree-sitter's `grammar.json` format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[allow(clippy::upper_case_acronyms, non_camel_case_types)]
pub enum RawRule {
    BLANK,
    STRING {
        value: String,
    },
    PATTERN {
        value: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        flags: Option<String>,
    },
    SYMBOL {
        name: String,
    },
    SEQ {
        members: Vec<RawRule>,
    },
    CHOICE {
        members: Vec<RawRule>,
    },
    REPEAT {
        content: Box<RawRule>,
    },
    REPEAT1 {
        content: Box<RawRule>,
    },
    FIELD {
        name: String,
        content: Box<RawRule>,
    },
    ALIAS {
        content: Box<RawRule>,
        value: String,
        named: bool,
    },
    TOKEN {
        content: Box<RawRule>,
    },
    IMMEDIATE_TOKEN {
        content: Box<RawRule>,
    },
    PREC {
        value: RawPrecedence,
        content: Box<RawRule>,
    },
    PREC_LEFT {
        value: RawPrecedence,
        content: Box<RawRule>,
    },
    PREC_RIGHT {
        value: RawPrecedence,
        content: Box<RawRule>,
    },
    PREC_DYNAMIC {
        value: i32,
        content: Box<RawRule>,
    },
    RESERVED {
        context_name: String,
        content: Box<RawRule>,
    },
}

/// Raw precedence value (integer or named).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RawPrecedence {
    Integer(i32),
    Name(String),
}

/// Raw entry in a precedence ordering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[allow(clippy::upper_case_acronyms)]
pub enum RawPrecedenceEntry {
    STRING { value: String },
    SYMBOL { name: String },
}
