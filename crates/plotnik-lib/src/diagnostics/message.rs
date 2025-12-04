//! Diagnostic message types and related structures.

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

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Error => write!(f, "error"),
            Severity::Warning => write!(f, "warning"),
        }
    }
}

/// The stage at which a diagnostic occurred.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticStage {
    /// Lexing/parsing diagnostics (syntax structure)
    #[default]
    Parse,
    /// Semantic validation diagnostics (mixed alternations, etc.)
    Validate,
    /// Name resolution diagnostics (undefined/duplicate references)
    Resolve,
    /// Escape analysis diagnostics (infinite recursion)
    Escape,
}

impl std::fmt::Display for DiagnosticStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiagnosticStage::Parse => write!(f, "parse"),
            DiagnosticStage::Validate => write!(f, "validate"),
            DiagnosticStage::Resolve => write!(f, "resolve"),
            DiagnosticStage::Escape => write!(f, "escape"),
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

fn serialize_text_range<S: Serializer>(range: &TextRange, s: S) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeStruct;
    let mut state = s.serialize_struct("TextRange", 2)?;
    state.serialize_field("start", &u32::from(range.start()))?;
    state.serialize_field("end", &u32::from(range.end()))?;
    state.end()
}

/// A diagnostic message with location, message, severity, and optional fix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiagnosticMessage {
    pub severity: Severity,
    pub stage: DiagnosticStage,
    #[serde(serialize_with = "serialize_text_range")]
    pub range: TextRange,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<Fix>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<RelatedInfo>,
}

impl DiagnosticMessage {
    /// Create an error diagnostic.
    pub fn error(range: TextRange, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            stage: DiagnosticStage::default(),
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
            stage: DiagnosticStage::default(),
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
    pub fn with_stage(mut self, stage: DiagnosticStage) -> Self {
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

impl std::fmt::Display for DiagnosticMessage {
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

impl std::error::Error for DiagnosticMessage {}