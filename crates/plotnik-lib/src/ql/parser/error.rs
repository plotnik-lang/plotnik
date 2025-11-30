//! Syntax error types and rendering utilities.

use annotate_snippets::{AnnotationKind, Level, Renderer, Snippet};
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

/// A syntax error with location, message, and optional fix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyntaxError {
    #[serde(serialize_with = "serialize_text_range")]
    pub range: TextRange,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<Fix>,
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
        }
    }

    pub fn with_fix(range: TextRange, message: impl Into<String>, fix: Fix) -> Self {
        Self {
            range,
            message: message.into(),
            fix: Some(fix),
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
        Ok(())
    }
}

impl std::error::Error for SyntaxError {}

/// Render syntax errors using annotate-snippets for nice diagnostic output.
pub fn render_errors(source: &str, errors: &[SyntaxError]) -> String {
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

        let report = &[Level::ERROR.primary_title(&err.message).element(
            Snippet::source(source)
                .line_start(1)
                .annotation(AnnotationKind::Primary.span(start..end)),
        )];

        if i > 0 {
            output.push('\n');
        }
        output.push_str(&renderer.render(report).to_string());

        if let Some(fix) = &err.fix {
            output.push_str(&format!("\n  help: {}", fix.description));
            output.push_str(&format!("\n  suggestion: `{}`", fix.replacement));
        }
    }

    output
}