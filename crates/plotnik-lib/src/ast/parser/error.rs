//! Diagnostic types and rendering utilities.

use annotate_snippets::{AnnotationKind, Group, Level, Patch, Renderer, Snippet};
use rowan::{TextRange, TextSize};
use serde::{Serialize, Serializer};

/// Severity level of a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    #[default]
    Error,
    Warning,
}

impl Severity {
    fn to_level(self) -> Level<'static> {
        match self {
            Severity::Error => Level::ERROR,
            Severity::Warning => Level::WARNING,
        }
    }
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Error => write!(f, "error"),
            Severity::Warning => write!(f, "warning"),
        }
    }
}

/// The stage at which a diagnostic occurred (internal implementation detail).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ErrorStage {
    /// Lexing/parsing errors (syntax structure)
    #[default]
    Parse,
    /// Semantic validation errors (mixed alternations, etc.)
    Validate,
    /// Name resolution errors (undefined/duplicate references)
    Resolve,
    /// Escape analysis errors (infinite recursion)
    Escape,
}

impl std::fmt::Display for ErrorStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ErrorStage::Parse => write!(f, "parse"),
            ErrorStage::Validate => write!(f, "validate"),
            ErrorStage::Resolve => write!(f, "resolve"),
            ErrorStage::Escape => write!(f, "escape"),
        }
    }
}

/// A suggested fix for a diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Fix {
    pub replacement: String,
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

/// Related location information for a diagnostic.
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

/// A diagnostic with location, message, severity, and optional fix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub stage: ErrorStage,
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

impl Diagnostic {
    /// Create an error diagnostic.
    pub fn error(range: TextRange, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            stage: ErrorStage::default(),
            range,
            message: message.into(),
            fix: None,
            related: Vec::new(),
        }
    }

    /// Create a warning diagnostic.
    pub fn warning(range: TextRange, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            stage: ErrorStage::default(),
            range,
            message: message.into(),
            fix: None,
            related: Vec::new(),
        }
    }

    /// Create an error at a zero-width position.
    pub fn error_at(offset: TextSize, message: impl Into<String>) -> Self {
        Self::error(TextRange::empty(offset), message)
    }

    /// Create a warning at a zero-width position.
    pub fn warning_at(offset: TextSize, message: impl Into<String>) -> Self {
        Self::warning(TextRange::empty(offset), message)
    }

    /// Set the pipeline stage.
    pub fn with_stage(mut self, stage: ErrorStage) -> Self {
        self.stage = stage;
        self
    }

    /// Add a fix suggestion.
    pub fn with_fix(mut self, fix: Fix) -> Self {
        self.fix = Some(fix);
        self
    }

    /// Add a related location.
    pub fn with_related(mut self, related: RelatedInfo) -> Self {
        self.related.push(related);
        self
    }

    /// Add multiple related locations.
    pub fn with_related_many(mut self, related: impl IntoIterator<Item = RelatedInfo>) -> Self {
        self.related.extend(related);
        self
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }

    pub fn is_warning(&self) -> bool {
        self.severity == Severity::Warning
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} at {}..{}: {}",
            self.severity,
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

impl std::error::Error for Diagnostic {}

/// Options for rendering diagnostics.
#[derive(Debug, Clone, Copy)]
pub struct RenderOptions {
    pub colored: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self { colored: true }
    }
}

impl RenderOptions {
    pub fn plain() -> Self {
        Self { colored: false }
    }

    pub fn colored() -> Self {
        Self { colored: true }
    }
}

/// Render diagnostics using annotate-snippets.
pub fn render_diagnostics(
    source: &str,
    diagnostics: &[Diagnostic],
    path: Option<&str>,
    options: RenderOptions,
) -> String {
    if diagnostics.is_empty() {
        return String::new();
    }

    let renderer = if options.colored {
        Renderer::styled()
    } else {
        Renderer::plain()
    };

    let mut output = String::new();

    for (i, diag) in diagnostics.iter().enumerate() {
        let start: usize = diag.range.start().into();
        let end: usize = diag.range.end().into();
        let end = if start == end {
            (start + 1).min(source.len())
        } else {
            end
        };

        let mut snippet = Snippet::source(source).line_start(1).annotation(
            AnnotationKind::Primary
                .span(start..end)
                .label(&diag.message),
        );

        if let Some(p) = path {
            snippet = snippet.path(p);
        }

        for related in &diag.related {
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

        let level = diag.severity.to_level();
        let title_group = level.primary_title(&diag.message).element(snippet);

        let mut report: Vec<Group> = vec![title_group];

        if let Some(fix) = &diag.fix {
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

// Backwards compatibility aliases
pub type SyntaxError = Diagnostic;

pub fn render_errors(source: &str, errors: &[Diagnostic], path: Option<&str>) -> String {
    render_diagnostics(source, errors, path, RenderOptions::plain())
}
