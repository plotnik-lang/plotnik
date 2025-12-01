//! Syntax error types and rendering utilities.

use annotate_snippets::{AnnotationKind, Group, Level, Patch, Renderer, Snippet};
use rowan::{TextRange, TextSize};
use serde::{Serialize, Serializer};

/// A suggested fix for a syntax error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Fix {
    /// The text to replace the error span with.
    pub replacement: String,
    /// Human-readable description of what the fix does.
    pub description: String,
}

impl Fix {
    pub fn new(replacement: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            replacement: replacement.into(),
            description: description.into(),
        }
    }
}

/// Related location information for a syntax error.
/// Used to point to where a construct started (e.g., unclosed delimiter).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RelatedInfo {
    #[serde(serialize_with = "serialize_text_range")]
    pub range: TextRange,
    pub message: String,
}

impl RelatedInfo {
    pub fn new(range: TextRange, message: impl Into<String>) -> Self {
        Self {
            range,
            message: message.into(),
        }
    }
}

/// A syntax error with location, message, and optional fix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyntaxError {
    #[serde(serialize_with = "serialize_text_range")]
    pub range: TextRange,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<Fix>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<RelatedInfo>,
}

fn serialize_text_range<S: Serializer>(range: &TextRange, s: S) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeStruct;
    let mut state = s.serialize_struct("TextRange", 2)?;
    state.serialize_field("start", &u32::from(range.start()))?;
    state.serialize_field("end", &u32::from(range.end()))?;
    state.end()
}

impl SyntaxError {
    pub fn new(range: TextRange, message: impl Into<String>) -> Self {
        Self {
            range,
            message: message.into(),
            fix: None,
            related: Vec::new(),
        }
    }

    pub fn with_fix(range: TextRange, message: impl Into<String>, fix: Fix) -> Self {
        Self {
            range,
            message: message.into(),
            fix: Some(fix),
            related: Vec::new(),
        }
    }

    pub fn with_related(
        range: TextRange,
        message: impl Into<String>,
        related: RelatedInfo,
    ) -> Self {
        Self {
            range,
            message: message.into(),
            fix: None,
            related: vec![related],
        }
    }

    pub fn with_related_many(
        range: TextRange,
        message: impl Into<String>,
        related: Vec<RelatedInfo>,
    ) -> Self {
        Self {
            range,
            message: message.into(),
            fix: None,
            related,
        }
    }

    pub fn at_offset(offset: TextSize, message: impl Into<String>) -> Self {
        Self::new(TextRange::empty(offset), message)
    }
}

impl std::fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "error at {}..{}: {}",
            u32::from(self.range.start()),
            u32::from(self.range.end()),
            self.message
        )?;
        if let Some(fix) = &self.fix {
            write!(f, " (fix: {})", fix.description)?;
        }
        for related in &self.related {
            write!(
                f,
                " (related: {} at {}..{})",
                related.message,
                u32::from(related.range.start()),
                u32::from(related.range.end())
            )?;
        }
        Ok(())
    }
}

impl std::error::Error for SyntaxError {}

/// Render syntax errors using annotate-snippets for nice diagnostic output.
pub fn render_errors(source: &str, errors: &[SyntaxError], path: Option<&str>) -> String {
    if errors.is_empty() {
        return String::new();
    }

    let renderer = Renderer::plain();
    let mut output = String::new();

    for (i, err) in errors.iter().enumerate() {
        let start: usize = err.range.start().into();
        let end: usize = err.range.end().into();
        // For zero-width spans, extend to at least 1 char for visibility
        let end = if start == end {
            (start + 1).min(source.len())
        } else {
            end
        };

        let mut snippet = Snippet::source(source)
            .line_start(1)
            .annotation(AnnotationKind::Primary.span(start..end).label(&err.message));

        if let Some(p) = path {
            snippet = snippet.path(p);
        }

        // Add related spans
        for related in &err.related {
            let rel_start: usize = related.range.start().into();
            let rel_end: usize = related.range.end().into();
            let rel_end = if rel_start == rel_end {
                (rel_start + 1).min(source.len())
            } else {
                rel_end
            };
            snippet = snippet.annotation(
                AnnotationKind::Context
                    .span(rel_start..rel_end)
                    .label(&related.message),
            );
        }

        let error_group = Level::ERROR.primary_title(&err.message).element(snippet);

        let mut report: Vec<Group> = vec![error_group];

        if let Some(fix) = &err.fix {
            report.push(
                Level::HELP.secondary_title(&fix.description).element(
                    Snippet::source(source)
                        .line_start(1)
                        .patch(Patch::new(start..end, &fix.replacement)),
                ),
            );
        }

        if i > 0 {
            output.push('\n');
        }
        output.push_str(&renderer.render(&report).to_string());
    }

    output
}
