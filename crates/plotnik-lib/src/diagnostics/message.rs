//! Diagnostic message types and related structures.

use rowan::TextRange;

/// Severity level of a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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

/// A suggested fix for a diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    pub(crate) replacement: String,
    pub(crate) description: String,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelatedInfo {
    pub(crate) range: TextRange,
    pub(crate) message: String,
}

impl RelatedInfo {
    pub fn new(range: TextRange, message: impl Into<String>) -> Self {
        Self {
            range,
            message: message.into(),
        }
    }
}

/// A diagnostic message with location, message, severity, and optional fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiagnosticMessage {
    pub(crate) severity: Severity,
    pub(crate) range: TextRange,
    pub(crate) message: String,
    pub(crate) fix: Option<Fix>,
    pub(crate) related: Vec<RelatedInfo>,
}

impl DiagnosticMessage {
    pub(crate) fn error(range: TextRange, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            range,
            message: message.into(),
            fix: None,
            related: Vec::new(),
        }
    }

    pub(crate) fn warning(range: TextRange, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            range,
            message: message.into(),
            fix: None,
            related: Vec::new(),
        }
    }

    pub(crate) fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }

    pub(crate) fn is_warning(&self) -> bool {
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
