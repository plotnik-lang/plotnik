//! Canonical serializable form of diagnostics.
//!
//! The terminal renderer, CLI `--json` output, WASM API, and future LSP are
//! all views over this schema. Field semantics:
//!
//! - `code`: stable snake_case identifier derived from [`DiagnosticKind`]
//! - `line`/`column`: 1-based; columns count Unicode scalar values
//! - `offset`: byte offset into the source

use serde::Serialize;

use super::message::{DiagnosticKind, Severity};
use super::{SourceMap, message};

#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    pub code: DiagnosticKind,
    pub severity: Severity,
    pub message: String,
    pub span: Span,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<Related>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<Fix>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hints: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Span {
    /// File path, or `<query>`/`<stdin>` for non-file sources.
    pub file: String,
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Clone, Serialize)]
pub struct Position {
    pub line: u32,
    pub column: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Related {
    pub message: String,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct Fix {
    pub description: String,
    pub replacement: String,
}

impl Diagnostic {
    pub(crate) fn from_diagnostic(msg: &message::Diagnostic, sources: &SourceMap) -> Self {
        Self {
            code: msg.kind,
            severity: msg.severity(),
            message: msg.message.clone(),
            span: json_span(sources, msg.span),
            related: msg
                .related
                .iter()
                .map(|r| Related {
                    message: r.message.clone(),
                    span: json_span(sources, r.span),
                })
                .collect(),
            fix: msg.fix.as_ref().map(|f| Fix {
                description: f.description.clone(),
                replacement: f.replacement.clone(),
            }),
            hints: msg.hints.clone(),
        }
    }
}

fn json_span(sources: &SourceMap, span: super::Span) -> Span {
    let content = sources.content(span.source);
    Span {
        file: sources.kind(span.source).display_name().to_string(),
        start: json_position(content, span.range.start().into()),
        end: json_position(content, span.range.end().into()),
    }
}

fn json_position(content: &str, offset: usize) -> Position {
    let prefix = &content[..offset];
    let line_start = prefix.rfind('\n').map_or(0, |i| i + 1);
    Position {
        line: prefix.matches('\n').count() as u32 + 1,
        column: prefix[line_start..].chars().count() as u32 + 1,
        offset: offset as u32,
    }
}
