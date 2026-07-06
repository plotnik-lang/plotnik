//! Query-shaped rendering for parsed source trees.
//!
//! Renders a tree-sitter tree as indented S-expressions that are also valid
//! Plotnik patterns, so any line of the dump can be copy-pasted into a query:
//! leaf text becomes a string predicate (`(identifier == "x")`), a leaf whose
//! text equals its kind collapses to `(this)`, and nodes the grammar allows
//! children for stay bare (`(statement_block)`) — a predicate there would be
//! rejected by the analyzer's leaf check.
//!
//! Output is a list of typed chunks so the playground can colorize without
//! re-parsing; the CLI concatenates the chunk texts.

use std::fmt::Write as _;

use serde::Serialize;

use crate::core::grammar::Grammar;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DumpChunkKind {
    /// Whitespace: indentation and newlines.
    Text,
    /// Structural punctuation: parens, `: ` after a field, ` == `.
    Punct,
    /// A node kind.
    Kind,
    /// A field name.
    Field,
    /// A quoted string: predicate values and raw-mode anonymous tokens.
    String,
    /// Trailing `; "…"` comment carrying text a pattern can't express (ERROR).
    Comment,
}

#[derive(Debug, Clone, Serialize)]
pub struct DumpChunk {
    pub kind: DumpChunkKind,
    pub text: String,
}

/// One emission for the iterative dumper's work stack.
enum Step<'t> {
    Node {
        node: tree_sitter::Node<'t>,
        field: Option<&'t str>,
        depth: usize,
    },
    Chunk(DumpChunkKind, &'static str),
}

/// Dump a parsed tree as chunks of Plotnik pattern syntax.
///
/// The source tree is untrusted and can nest past any native-stack budget, so
/// the walk uses an explicit work stack rather than native recursion.
pub fn dump_tree_chunks(
    tree: &tree_sitter::Tree,
    source: &str,
    grammar: &Grammar,
    raw: bool,
) -> Vec<DumpChunk> {
    let mut out = ChunkWriter::default();
    let mut stack = vec![Step::Node {
        node: tree.root_node(),
        field: None,
        depth: 0,
    }];

    while let Some(step) = stack.pop() {
        let (node, field, depth) = match step {
            Step::Chunk(kind, text) => {
                out.push(kind, text);
                continue;
            }
            Step::Node { node, field, depth } => (node, field, depth),
        };

        // Anonymous nodes are dropped unless `raw`. Children are pre-filtered
        // below, so this only guards a (hypothetical) anonymous root.
        if !raw && !node.is_named() {
            continue;
        }

        if depth > 0 {
            out.push(DumpChunkKind::Text, "  ".repeat(depth));
        }
        if let Some(f) = field {
            out.push(DumpChunkKind::Field, f);
            out.push(DumpChunkKind::Punct, ": ");
        }

        let children = collect_children(node, raw);
        if children.is_empty() {
            dump_leaf(&mut out, node, source, grammar);
            continue;
        }

        out.push(DumpChunkKind::Punct, "(");
        out.push(DumpChunkKind::Kind, node.kind());
        // Queue, in source order, a newline + each child, then the closing paren.
        let mut deferred = Vec::with_capacity(children.len() * 2 + 1);
        for (child, child_field) in children {
            deferred.push(Step::Chunk(DumpChunkKind::Text, "\n"));
            deferred.push(Step::Node {
                node: child,
                field: child_field,
                depth: depth + 1,
            });
        }
        deferred.push(Step::Chunk(DumpChunkKind::Punct, ")"));
        stack.extend(deferred.into_iter().rev());
    }

    out.push(DumpChunkKind::Text, "\n");
    out.finish()
}

/// Plain-text dump: the chunk texts concatenated (what the CLI prints).
pub fn dump_tree_text(
    tree: &tree_sitter::Tree,
    source: &str,
    grammar: &Grammar,
    raw: bool,
) -> String {
    dump_tree_chunks(tree, source, grammar, raw)
        .into_iter()
        .map(|chunk| chunk.text)
        .collect()
}

fn dump_leaf(out: &mut ChunkWriter, node: tree_sitter::Node<'_>, source: &str, grammar: &Grammar) {
    let kind = node.kind();
    let text = node
        .utf8_text(source.as_bytes())
        .unwrap_or("<invalid utf8>");

    // Raw mode surfaces anonymous nodes; as a pattern those are bare string
    // tokens (`"("`), matched by kind.
    if !node.is_named() {
        out.push(DumpChunkKind::String, quoted(kind));
        return;
    }

    if kind == "ERROR" {
        out.push(DumpChunkKind::Punct, "(");
        out.push(DumpChunkKind::Kind, "ERROR");
        out.push(DumpChunkKind::Punct, ")");
        // `(ERROR)` accepts neither children nor predicates in a query, but
        // the unparsed text is the payload here — carry it in a comment.
        if !text.is_empty() {
            out.push(DumpChunkKind::Text, " ");
            out.push(DumpChunkKind::Comment, format!("; {}", quoted(text)));
        }
        return;
    }

    // Keyword-ish leaf (`this`, `true`): the kind alone pins the text.
    if text == kind {
        out.push(DumpChunkKind::Punct, "(");
        out.push(DumpChunkKind::Kind, kind);
        out.push(DumpChunkKind::Punct, ")");
        return;
    }

    // A text predicate is only valid where the grammar proves a leaf shape
    // (the analyzer's PredicateOnNonLeaf rule); anything else — empty
    // containers, kinds the grammar can't resolve — stays bare.
    let leaf = grammar
        .resolve_named_node(kind)
        .is_some_and(|id| !grammar.has_declared_child_structure(id));

    out.push(DumpChunkKind::Punct, "(");
    out.push(DumpChunkKind::Kind, kind);
    if leaf {
        out.push(DumpChunkKind::Punct, " == ");
        out.push(DumpChunkKind::String, quoted(text));
    }
    out.push(DumpChunkKind::Punct, ")");
}

/// Quote and escape text as a Plotnik string literal. The escape set matches
/// `compiler/parse/strings.rs::unescape`, so the dump round-trips.
fn quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            c if c.is_control() => {
                let _ = write!(out, "\\u{{{:04x}}}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[derive(Default)]
struct ChunkWriter {
    chunks: Vec<DumpChunk>,
}

impl ChunkWriter {
    fn push(&mut self, kind: DumpChunkKind, text: impl Into<String>) {
        self.chunks.push(DumpChunk {
            kind,
            text: text.into(),
        });
    }

    fn finish(self) -> Vec<DumpChunk> {
        self.chunks
    }
}

/// Collect a node's children, keeping anonymous ones only in `raw` mode,
/// paired with their field names.
fn collect_children<'t>(
    node: tree_sitter::Node<'t>,
    raw: bool,
) -> Vec<(tree_sitter::Node<'t>, Option<&'t str>)> {
    let mut cursor = node.walk();
    let mut result = Vec::new();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            if raw || child.is_named() {
                result.push((child, cursor.field_name()));
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    result
}
