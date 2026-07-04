//! Tree-shape rendering of a tree-sitter grammar.
//!
//! Where `grammar.json` describes how *text parses*, a [`TreeGrammar`] describes
//! how *trees are shaped* — the artifact a query author needs. It is lowered once
//! from the grammar pipeline (see [`lower`]) and retained on
//! [`Grammar`](super::Grammar); the CLI renders the whole document with
//! [`TreeGrammar::dump`], and diagnostics render fragments, all from this one model.
//!
//! Every definition body belongs to exactly one *register*, each with a distinct
//! surface syntax so the three are lexically impossible to confuse:
//!
//! - **pattern** — node/hidden shapes, in query-flavored pattern notation;
//! - **type** — categories (tree-sitter supertypes), `|`-unions of member kinds;
//! - **text** — tokens, a synthesized string literal or regex.

mod dump;
mod layout;
mod lexical;
mod lower;

#[cfg(test)]
mod lexical_tests;

use std::fmt;

pub use dump::{DEFAULT_WIDTH, DumpOptions};
pub(in crate::core::grammar) use lower::{attach_node_shapes, lower};

/// A grammar rendered as tree shapes: definitions in grammar order, the extras
/// list, and the root kind name (for the `; root` annotation).
#[derive(Debug, Clone, Default)]
pub struct TreeGrammar {
    name: String,
    defs: Vec<Def>,
    extras: Vec<Shape>,
    root: Option<String>,
}

impl TreeGrammar {
    /// Grammar name (e.g. `"json"`).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Definitions in grammar (declaration) order.
    pub fn defs(&self) -> &[Def] {
        &self.defs
    }
}

/// One top-level definition: a node shape, a hidden rule, a category, a token,
/// an external token, or an alias-only kind.
#[derive(Debug, Clone)]
pub struct Def {
    /// Display name. Categories are de-underscored; the `#` is added at render time.
    pub name: String,
    pub kind: DefKind,
    pub extra: bool,
    pub root: bool,
    pub body: Body,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefKind {
    /// A queryable named node — pattern register.
    Node,
    /// A hidden rule (underscore-prefixed, inlined, or auxiliary) — pattern register.
    Hidden,
    /// A category (tree-sitter supertype) — type register.
    Category,
    /// A leaf token — text register.
    Token,
    /// An external scanner token — no rule body.
    External,
    /// A kind that exists only as an alias of the named underlying rule.
    AliasOf(String),
}

#[derive(Debug, Clone)]
pub enum Body {
    /// Pattern register: a node/hidden shape in query-flavored notation.
    Pattern(Shape),
    /// Type register: the visible subtype closure of a category.
    Category(Vec<NodeRef>),
    /// Text register: a synthesized token.
    Token(TokenText),
    /// No body (external scanner token).
    None,
}

/// A pattern-register shape: a fragment of query-flavored pattern notation.
/// Most constructs are real query syntax; categories (`name#`), hidden splices
/// (`_name`), and inline token text describe structure and are not written
/// literally in a query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shape {
    /// A queryable position: `(identifier)`, `"+"`, or `(value#)`.
    Node(NodeRef),
    /// An inline anonymous token with no queryable name — an extras separator or
    /// an auxiliary regex token spliced into a shape. Rendered as `/regex/`.
    Token(TokenText),
    /// A bare hidden reference: `_call_signature`. Its children splice in here; it
    /// is not a node a query can match.
    Splice(String),
    /// An ordered sibling sequence: `{...}`.
    Seq(Vec<Shape>),
    /// An alternation, first match wins: `[...]`.
    Choice(Vec<Shape>),
    /// A quantified shape: `?`, `*`, `+`.
    Quantified(Box<Shape>, Quant),
    /// A field constraint: `name: shape`.
    Field(String, Box<Shape>),
    /// An empty body (a rule that matches only the empty string).
    Empty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quant {
    Optional,
    Star,
    Plus,
}

impl Quant {
    fn marker(self) -> char {
        match self {
            Self::Optional => '?',
            Self::Star => '*',
            Self::Plus => '+',
        }
    }
}

/// A reference to a node kind in a pattern: named node, anonymous node, or category.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeRef {
    pub name: String,
    pub named: bool,
    /// A category reference (tree-sitter supertype) — renders with a trailing `#`.
    pub category: bool,
}

impl fmt::Display for NodeRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.category {
            write!(f, "({}#)", self.name)
        } else if self.named {
            write!(f, "({})", self.name)
        } else {
            write!(f, "\"{}\"", escape_literal(&self.name))
        }
    }
}

/// A token's synthesized text — a string literal or a regex.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenText {
    Str(String),
    Regex(String),
}

impl fmt::Display for TokenText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Str(value) => write!(f, "\"{}\"", escape_literal(value)),
            Self::Regex(value) => write!(f, "/{value}/"),
        }
    }
}

/// Escape a string for display inside a `"..."` literal.
fn escape_literal(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}
