//! Syntax error types and rendering utilities.

use annotate_snippets::{AnnotationKind, Level, Renderer, Snippet};
use rowan::{TextRange, TextSize};

/// A syntax error with location and message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxError {
    pub range: TextRange,
    pub message: String,
}

impl SyntaxError {
    pub fn new(range: TextRange, message: impl Into<String>) -> Self {
        Self {
            range,
            message: message.into(),
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
        )
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
    }

    output
}