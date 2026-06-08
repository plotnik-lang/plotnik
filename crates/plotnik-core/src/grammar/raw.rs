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

    /// Decode a raw grammar from build-time postcard bytes.
    pub fn from_postcard(bytes: &[u8]) -> Result<Self, GrammarError> {
        postcard::from_bytes::<RawGrammarPostcard>(bytes)
            .map(Into::into)
            .map_err(GrammarError::Postcard)
    }

    /// Encode this raw grammar as postcard bytes.
    pub fn to_postcard(&self) -> Result<Vec<u8>, GrammarError> {
        postcard::to_allocvec(&RawGrammarPostcard::from(self)).map_err(GrammarError::Postcard)
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RawGrammarPostcard {
    name: String,
    rules: Vec<(String, RawRulePostcard)>,
    extras: Vec<RawRulePostcard>,
    precedences: Vec<Vec<RawPrecedenceEntryPostcard>>,
    conflicts: Vec<Vec<String>>,
    externals: Vec<RawRulePostcard>,
    inline: Vec<String>,
    supertypes: Vec<String>,
    word: Option<String>,
    reserved: Vec<(String, Vec<RawRulePostcard>)>,
    inherits: Option<String>,
}

impl From<&RawGrammar> for RawGrammarPostcard {
    fn from(raw: &RawGrammar) -> Self {
        Self {
            name: raw.name.clone(),
            rules: raw
                .rules
                .iter()
                .map(|(name, rule)| (name.clone(), RawRulePostcard::from(rule)))
                .collect(),
            extras: raw.extras.iter().map(RawRulePostcard::from).collect(),
            precedences: raw
                .precedences
                .iter()
                .map(|entries| {
                    entries
                        .iter()
                        .map(RawPrecedenceEntryPostcard::from)
                        .collect()
                })
                .collect(),
            conflicts: raw.conflicts.clone(),
            externals: raw.externals.iter().map(RawRulePostcard::from).collect(),
            inline: raw.inline.clone(),
            supertypes: raw.supertypes.clone(),
            word: raw.word.clone(),
            reserved: raw
                .reserved
                .iter()
                .map(|(name, rules)| {
                    (
                        name.clone(),
                        rules.iter().map(RawRulePostcard::from).collect(),
                    )
                })
                .collect(),
            inherits: raw.inherits.clone(),
        }
    }
}

impl From<RawGrammarPostcard> for RawGrammar {
    fn from(raw: RawGrammarPostcard) -> Self {
        Self {
            name: raw.name,
            rules: raw
                .rules
                .into_iter()
                .map(|(name, rule)| (name, RawRule::from(rule)))
                .collect(),
            extras: raw.extras.into_iter().map(RawRule::from).collect(),
            precedences: raw
                .precedences
                .into_iter()
                .map(|entries| entries.into_iter().map(RawPrecedenceEntry::from).collect())
                .collect(),
            conflicts: raw.conflicts,
            externals: raw.externals.into_iter().map(RawRule::from).collect(),
            inline: raw.inline,
            supertypes: raw.supertypes,
            word: raw.word,
            reserved: raw
                .reserved
                .into_iter()
                .map(|(name, rules)| (name, rules.into_iter().map(RawRule::from).collect()))
                .collect(),
            inherits: raw.inherits,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum RawRulePostcard {
    Blank,
    String(String),
    Pattern {
        value: String,
        flags: Option<String>,
    },
    Symbol(String),
    Seq(Vec<RawRulePostcard>),
    Choice(Vec<RawRulePostcard>),
    Repeat(Box<RawRulePostcard>),
    Repeat1(Box<RawRulePostcard>),
    Field {
        name: String,
        content: Box<RawRulePostcard>,
    },
    Alias {
        content: Box<RawRulePostcard>,
        value: String,
        named: bool,
    },
    Token(Box<RawRulePostcard>),
    ImmediateToken(Box<RawRulePostcard>),
    Prec {
        value: RawPrecedencePostcard,
        content: Box<RawRulePostcard>,
    },
    PrecLeft {
        value: RawPrecedencePostcard,
        content: Box<RawRulePostcard>,
    },
    PrecRight {
        value: RawPrecedencePostcard,
        content: Box<RawRulePostcard>,
    },
    PrecDynamic {
        value: i32,
        content: Box<RawRulePostcard>,
    },
    Reserved {
        context_name: String,
        content: Box<RawRulePostcard>,
    },
}

impl From<&RawRule> for RawRulePostcard {
    fn from(rule: &RawRule) -> Self {
        match rule {
            RawRule::BLANK => Self::Blank,
            RawRule::STRING { value } => Self::String(value.clone()),
            RawRule::PATTERN { value, flags } => Self::Pattern {
                value: value.clone(),
                flags: flags.clone(),
            },
            RawRule::SYMBOL { name } => Self::Symbol(name.clone()),
            RawRule::SEQ { members } => Self::Seq(members.iter().map(Self::from).collect()),
            RawRule::CHOICE { members } => Self::Choice(members.iter().map(Self::from).collect()),
            RawRule::REPEAT { content } => Self::Repeat(Box::new(Self::from(content.as_ref()))),
            RawRule::REPEAT1 { content } => Self::Repeat1(Box::new(Self::from(content.as_ref()))),
            RawRule::FIELD { name, content } => Self::Field {
                name: name.clone(),
                content: Box::new(Self::from(content.as_ref())),
            },
            RawRule::ALIAS {
                content,
                value,
                named,
            } => Self::Alias {
                content: Box::new(Self::from(content.as_ref())),
                value: value.clone(),
                named: *named,
            },
            RawRule::TOKEN { content } => Self::Token(Box::new(Self::from(content.as_ref()))),
            RawRule::IMMEDIATE_TOKEN { content } => {
                Self::ImmediateToken(Box::new(Self::from(content.as_ref())))
            }
            RawRule::PREC { value, content } => Self::Prec {
                value: RawPrecedencePostcard::from(value),
                content: Box::new(Self::from(content.as_ref())),
            },
            RawRule::PREC_LEFT { value, content } => Self::PrecLeft {
                value: RawPrecedencePostcard::from(value),
                content: Box::new(Self::from(content.as_ref())),
            },
            RawRule::PREC_RIGHT { value, content } => Self::PrecRight {
                value: RawPrecedencePostcard::from(value),
                content: Box::new(Self::from(content.as_ref())),
            },
            RawRule::PREC_DYNAMIC { value, content } => Self::PrecDynamic {
                value: *value,
                content: Box::new(Self::from(content.as_ref())),
            },
            RawRule::RESERVED {
                context_name,
                content,
            } => Self::Reserved {
                context_name: context_name.clone(),
                content: Box::new(Self::from(content.as_ref())),
            },
        }
    }
}

impl From<RawRulePostcard> for RawRule {
    fn from(rule: RawRulePostcard) -> Self {
        match rule {
            RawRulePostcard::Blank => Self::BLANK,
            RawRulePostcard::String(value) => Self::STRING { value },
            RawRulePostcard::Pattern { value, flags } => Self::PATTERN { value, flags },
            RawRulePostcard::Symbol(name) => Self::SYMBOL { name },
            RawRulePostcard::Seq(members) => Self::SEQ {
                members: members.into_iter().map(Self::from).collect(),
            },
            RawRulePostcard::Choice(members) => Self::CHOICE {
                members: members.into_iter().map(Self::from).collect(),
            },
            RawRulePostcard::Repeat(content) => Self::REPEAT {
                content: Box::new(Self::from(*content)),
            },
            RawRulePostcard::Repeat1(content) => Self::REPEAT1 {
                content: Box::new(Self::from(*content)),
            },
            RawRulePostcard::Field { name, content } => Self::FIELD {
                name,
                content: Box::new(Self::from(*content)),
            },
            RawRulePostcard::Alias {
                content,
                value,
                named,
            } => Self::ALIAS {
                content: Box::new(Self::from(*content)),
                value,
                named,
            },
            RawRulePostcard::Token(content) => Self::TOKEN {
                content: Box::new(Self::from(*content)),
            },
            RawRulePostcard::ImmediateToken(content) => Self::IMMEDIATE_TOKEN {
                content: Box::new(Self::from(*content)),
            },
            RawRulePostcard::Prec { value, content } => Self::PREC {
                value: RawPrecedence::from(value),
                content: Box::new(Self::from(*content)),
            },
            RawRulePostcard::PrecLeft { value, content } => Self::PREC_LEFT {
                value: RawPrecedence::from(value),
                content: Box::new(Self::from(*content)),
            },
            RawRulePostcard::PrecRight { value, content } => Self::PREC_RIGHT {
                value: RawPrecedence::from(value),
                content: Box::new(Self::from(*content)),
            },
            RawRulePostcard::PrecDynamic { value, content } => Self::PREC_DYNAMIC {
                value,
                content: Box::new(Self::from(*content)),
            },
            RawRulePostcard::Reserved {
                context_name,
                content,
            } => Self::RESERVED {
                context_name,
                content: Box::new(Self::from(*content)),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum RawPrecedencePostcard {
    Integer(i32),
    Name(String),
}

impl From<&RawPrecedence> for RawPrecedencePostcard {
    fn from(precedence: &RawPrecedence) -> Self {
        match precedence {
            RawPrecedence::Integer(value) => Self::Integer(*value),
            RawPrecedence::Name(name) => Self::Name(name.clone()),
        }
    }
}

impl From<RawPrecedencePostcard> for RawPrecedence {
    fn from(precedence: RawPrecedencePostcard) -> Self {
        match precedence {
            RawPrecedencePostcard::Integer(value) => Self::Integer(value),
            RawPrecedencePostcard::Name(name) => Self::Name(name),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum RawPrecedenceEntryPostcard {
    Name(String),
    Symbol(String),
}

impl From<&RawPrecedenceEntry> for RawPrecedenceEntryPostcard {
    fn from(entry: &RawPrecedenceEntry) -> Self {
        match entry {
            RawPrecedenceEntry::STRING { value } => Self::Name(value.clone()),
            RawPrecedenceEntry::SYMBOL { name } => Self::Symbol(name.clone()),
        }
    }
}

impl From<RawPrecedenceEntryPostcard> for RawPrecedenceEntry {
    fn from(entry: RawPrecedenceEntryPostcard) -> Self {
        match entry {
            RawPrecedenceEntryPostcard::Name(value) => Self::STRING { value },
            RawPrecedenceEntryPostcard::Symbol(name) => Self::SYMBOL { name },
        }
    }
}
