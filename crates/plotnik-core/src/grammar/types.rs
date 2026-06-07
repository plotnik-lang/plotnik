//! Grammar type definitions.

use serde::{Deserialize, Serialize};

/// Tree-sitter grammar plus derived metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Grammar {
    raw: RawGrammar,
}

impl Grammar {
    pub(crate) fn from_raw(raw: RawGrammar) -> Self {
        Self { raw }
    }

    /// Grammar name (e.g., "javascript", "rust").
    pub fn name(&self) -> &str {
        &self.raw.name
    }

    /// Production rules, preserving definition order.
    pub fn rules(&self) -> &[(String, Rule)] {
        &self.raw.rules
    }

    /// Extra/trivia nodes (comments, whitespace).
    pub fn extras(&self) -> &[Rule] {
        &self.raw.extras
    }

    /// External scanner tokens.
    pub fn externals(&self) -> &[Rule] {
        &self.raw.externals
    }

    /// Supertype rules.
    pub fn supertypes(&self) -> &[String] {
        &self.raw.supertypes
    }

    /// Rules to inline (hidden).
    pub fn inline(&self) -> &[String] {
        &self.raw.inline
    }

    pub(crate) fn raw(&self) -> &RawGrammar {
        &self.raw
    }
}

/// Direct parsed representation of tree-sitter grammar.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RawGrammar {
    /// Grammar name (e.g., "javascript", "rust").
    pub name: String,
    /// Production rules, preserving definition order.
    pub rules: Vec<(String, Rule)>,
    /// Extra/trivia nodes (comments, whitespace).
    #[serde(default)]
    pub extras: Vec<Rule>,
    /// Precedence orderings.
    #[serde(default)]
    pub precedences: Vec<Vec<PrecedenceEntry>>,
    /// Expected conflicts.
    #[serde(default)]
    pub conflicts: Vec<Vec<String>>,
    /// External scanner tokens.
    #[serde(default)]
    pub externals: Vec<Rule>,
    /// Rules to inline (hidden).
    #[serde(default)]
    pub inline: Vec<String>,
    /// Supertype rules.
    #[serde(default)]
    pub supertypes: Vec<String>,
    /// Keyword identifier rule.
    #[serde(default)]
    pub word: Option<String>,
    /// Reserved word contexts.
    #[serde(default)]
    pub reserved: Vec<(String, Vec<Rule>)>,
    /// Parent grammar name (for inheritance).
    #[serde(default)]
    pub inherits: Option<String>,
}

/// Grammar rule variants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Rule {
    /// Epsilon (empty match).
    Blank,
    /// Literal token.
    String(String),
    /// Regex token.
    Pattern {
        value: String,
        #[serde(default)]
        flags: Option<String>,
    },
    /// Reference to another rule.
    Symbol(String),
    /// Sequence of rules (must match in order).
    Seq(Vec<Rule>),
    /// Alternation (first matching wins).
    Choice(Vec<Rule>),
    /// Zero or more repetitions.
    Repeat(Box<Rule>),
    /// One or more repetitions.
    Repeat1(Box<Rule>),
    /// Named field.
    Field { name: String, content: Box<Rule> },
    /// Rename node.
    Alias {
        content: Box<Rule>,
        value: String,
        named: bool,
    },
    /// Force tokenization.
    Token(Box<Rule>),
    /// Immediate tokenization.
    ImmediateToken(Box<Rule>),
    /// Precedence.
    Prec {
        value: Precedence,
        content: Box<Rule>,
    },
    /// Left-associative precedence.
    PrecLeft {
        value: Precedence,
        content: Box<Rule>,
    },
    /// Right-associative precedence.
    PrecRight {
        value: Precedence,
        content: Box<Rule>,
    },
    /// Dynamic precedence.
    PrecDynamic { value: i32, content: Box<Rule> },
    /// Reserved word context.
    Reserved {
        context_name: String,
        content: Box<Rule>,
    },
}

/// Precedence value (numeric or named).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Precedence {
    Integer(i32),
    Name(String),
}

/// Entry in precedence ordering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PrecedenceEntry {
    /// Named precedence level.
    Name(String),
    /// Symbol reference.
    Symbol(String),
}
