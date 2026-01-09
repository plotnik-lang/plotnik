//! JSON deserialization for grammar.json files.
//!
//! Tree-sitter's grammar.json uses externally-tagged enums with `type` field.

use indexmap::IndexMap;
use serde::Deserialize;

use super::types::{Grammar, Precedence, PrecedenceEntry, Rule};

/// Error during grammar parsing.
#[derive(Debug)]
pub enum GrammarError {
    Json(serde_json::Error),
    Binary(postcard::Error),
}

impl std::fmt::Display for GrammarError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json(e) => write!(f, "JSON parse error: {e}"),
            Self::Binary(e) => write!(f, "binary decode error: {e}"),
        }
    }
}

impl std::error::Error for GrammarError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(e) => Some(e),
            Self::Binary(e) => Some(e),
        }
    }
}

impl Grammar {
    /// Parse grammar from JSON string.
    pub fn from_json(json: &str) -> Result<Self, GrammarError> {
        let raw: RawGrammar = serde_json::from_str(json).map_err(GrammarError::Json)?;
        Ok(raw.into())
    }
}

/// Raw grammar structure matching tree-sitter's JSON format.
#[derive(Debug, Deserialize)]
struct RawGrammar {
    name: String,
    rules: IndexMap<String, RawRule>,
    #[serde(default)]
    extras: Vec<RawRule>,
    #[serde(default)]
    precedences: Vec<Vec<RawPrecedenceEntry>>,
    #[serde(default)]
    conflicts: Vec<Vec<String>>,
    #[serde(default)]
    externals: Vec<RawRule>,
    #[serde(default, rename = "inline")]
    inline_rules: Vec<String>,
    #[serde(default)]
    supertypes: Vec<String>,
    #[serde(default)]
    word: Option<String>,
    #[serde(default)]
    reserved: IndexMap<String, Vec<RawRule>>,
    #[serde(default)]
    inherits: Option<String>,
}

impl From<RawGrammar> for Grammar {
    fn from(raw: RawGrammar) -> Self {
        // IndexMap preserves insertion order, which matches tree-sitter's definition order.
        // The entry rule is always first.
        Self {
            name: raw.name,
            rules: raw.rules.into_iter().map(|(k, v)| (k, v.into())).collect(),
            extras: raw.extras.into_iter().map(Into::into).collect(),
            precedences: raw
                .precedences
                .into_iter()
                .map(|v| v.into_iter().map(Into::into).collect())
                .collect(),
            conflicts: raw.conflicts,
            externals: raw.externals.into_iter().map(Into::into).collect(),
            inline: raw.inline_rules,
            supertypes: raw.supertypes,
            word: raw.word,
            reserved: raw
                .reserved
                .into_iter()
                .map(|(k, v)| (k, v.into_iter().map(Into::into).collect()))
                .collect(),
            inherits: raw.inherits,
        }
    }
}

/// Raw rule matching tree-sitter's JSON format.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
#[allow(clippy::upper_case_acronyms, non_camel_case_types)]
enum RawRule {
    BLANK,
    STRING {
        value: String,
    },
    PATTERN {
        value: String,
        #[serde(default)]
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

impl From<RawRule> for Rule {
    fn from(raw: RawRule) -> Self {
        #[allow(clippy::boxed_local)] // Fields are Box<RawRule>, output needs Box<Rule>
        fn conv(content: Box<RawRule>) -> Box<Rule> {
            Box::new(Rule::from(*content))
        }

        match raw {
            RawRule::BLANK => Rule::Blank,
            RawRule::STRING { value } => Rule::String(value),
            RawRule::PATTERN { value, flags } => Rule::Pattern { value, flags },
            RawRule::SYMBOL { name } => Rule::Symbol(name),
            RawRule::SEQ { members } => Rule::Seq(members.into_iter().map(Into::into).collect()),
            RawRule::CHOICE { members } => {
                Rule::Choice(members.into_iter().map(Into::into).collect())
            }
            RawRule::REPEAT { content } => Rule::Repeat(conv(content)),
            RawRule::REPEAT1 { content } => Rule::Repeat1(conv(content)),
            RawRule::FIELD { name, content } => Rule::Field {
                name,
                content: conv(content),
            },
            RawRule::ALIAS {
                content,
                value,
                named,
            } => Rule::Alias {
                content: conv(content),
                value,
                named,
            },
            RawRule::TOKEN { content } => Rule::Token(conv(content)),
            RawRule::IMMEDIATE_TOKEN { content } => Rule::ImmediateToken(conv(content)),
            RawRule::PREC { value, content } => Rule::Prec {
                value: value.into(),
                content: conv(content),
            },
            RawRule::PREC_LEFT { value, content } => Rule::PrecLeft {
                value: value.into(),
                content: conv(content),
            },
            RawRule::PREC_RIGHT { value, content } => Rule::PrecRight {
                value: value.into(),
                content: conv(content),
            },
            RawRule::PREC_DYNAMIC { value, content } => Rule::PrecDynamic {
                value,
                content: conv(content),
            },
            RawRule::RESERVED {
                context_name,
                content,
            } => Rule::Reserved {
                context_name,
                content: conv(content),
            },
        }
    }
}

/// Raw precedence value (can be integer or string).
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawPrecedence {
    Integer(i32),
    Name(String),
}

impl From<RawPrecedence> for Precedence {
    fn from(raw: RawPrecedence) -> Self {
        match raw {
            RawPrecedence::Integer(n) => Precedence::Integer(n),
            RawPrecedence::Name(s) => Precedence::Name(s),
        }
    }
}

/// Raw precedence entry (STRING or SYMBOL).
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
#[allow(clippy::upper_case_acronyms)]
enum RawPrecedenceEntry {
    STRING { value: String },
    SYMBOL { name: String },
}

impl From<RawPrecedenceEntry> for PrecedenceEntry {
    fn from(raw: RawPrecedenceEntry) -> Self {
        match raw {
            RawPrecedenceEntry::STRING { value } => PrecedenceEntry::Name(value),
            RawPrecedenceEntry::SYMBOL { name } => PrecedenceEntry::Symbol(name),
        }
    }
}
