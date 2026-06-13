//! Canonical serializable form of diagnostics.
//!
//! The terminal renderer, CLI `--json` output, WASM API, and future LSP are
//! all views over this schema. Field semantics:
//!
//! - `code`: stable snake_case identifier derived from [`DiagnosticKind`]
//! - `line`/`column`: 1-based; columns count Unicode scalar values
//! - `offset`: byte offset into the source

use rowan::TextRange;
use serde::Serialize;

use super::message::{DiagnosticKind, DiagnosticMessage, Severity};
use super::{SourceId, SourceMap};

#[derive(Debug, Clone, Serialize)]
pub struct JsonDiagnostic {
    pub code: DiagnosticKind,
    pub severity: Severity,
    pub message: String,
    pub span: JsonSpan,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<JsonRelated>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<JsonFix>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hints: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct JsonSpan {
    /// File path, or `<query>`/`<stdin>` for non-file sources.
    pub file: String,
    pub start: JsonPosition,
    pub end: JsonPosition,
}

#[derive(Debug, Clone, Serialize)]
pub struct JsonPosition {
    pub line: u32,
    pub column: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct JsonRelated {
    pub message: String,
    pub span: JsonSpan,
}

#[derive(Debug, Clone, Serialize)]
pub struct JsonFix {
    pub description: String,
    pub replacement: String,
}

impl JsonDiagnostic {
    pub(crate) fn from_message(msg: &DiagnosticMessage, sources: &SourceMap) -> Self {
        Self {
            code: msg.kind,
            severity: msg.severity(),
            message: msg.message.clone(),
            span: json_span(sources, msg.source, msg.range),
            related: msg
                .related
                .iter()
                .map(|r| JsonRelated {
                    message: r.message.clone(),
                    span: json_span(sources, r.span.source, r.span.range),
                })
                .collect(),
            fix: msg.fix.as_ref().map(|f| JsonFix {
                description: f.description.clone(),
                replacement: f.replacement.clone(),
            }),
            hints: msg.hints.clone(),
        }
    }
}

fn json_span(sources: &SourceMap, source: SourceId, range: TextRange) -> JsonSpan {
    let content = sources.content(source);
    JsonSpan {
        file: sources.kind(source).display_name().to_string(),
        start: json_position(content, range.start().into()),
        end: json_position(content, range.end().into()),
    }
}

fn json_position(content: &str, offset: usize) -> JsonPosition {
    let prefix = &content[..offset];
    let line_start = prefix.rfind('\n').map_or(0, |i| i + 1);
    JsonPosition {
        line: prefix.matches('\n').count() as u32 + 1,
        column: prefix[line_start..].chars().count() as u32 + 1,
        offset: offset as u32,
    }
}
