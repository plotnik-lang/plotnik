//! Compiler diagnostics infrastructure.
//!
//! This module provides types for collecting and rendering diagnostic messages.

mod message;
mod printer;

#[cfg(test)]
mod tests;

use rowan::TextRange;

pub use message::Severity;
pub use printer::DiagnosticsPrinter;

use message::{DiagnosticMessage, Fix, RelatedInfo};

/// Collection of diagnostic messages from parsing and analysis.
#[derive(Debug, Clone, Default)]
pub struct Diagnostics {
    messages: Vec<DiagnosticMessage>,
}

/// Builder for constructing a diagnostic message.
#[must_use = "diagnostic not emitted, call .emit()"]
pub struct DiagnosticBuilder<'a> {
    diagnostics: &'a mut Diagnostics,
    message: DiagnosticMessage,
}

impl Diagnostics {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
        }
    }

    pub fn error(&mut self, msg: impl Into<String>, range: TextRange) -> DiagnosticBuilder<'_> {
        DiagnosticBuilder {
            diagnostics: self,
            message: DiagnosticMessage::error(range, msg),
        }
    }

    pub fn warning(&mut self, msg: impl Into<String>, range: TextRange) -> DiagnosticBuilder<'_> {
        DiagnosticBuilder {
            diagnostics: self,
            message: DiagnosticMessage::warning(range, msg),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn has_errors(&self) -> bool {
        self.messages.iter().any(|d| d.is_error())
    }

    pub fn has_warnings(&self) -> bool {
        self.messages.iter().any(|d| d.is_warning())
    }

    pub fn error_count(&self) -> usize {
        self.messages.iter().filter(|d| d.is_error()).count()
    }

    pub fn warning_count(&self) -> usize {
        self.messages.iter().filter(|d| d.is_warning()).count()
    }

    pub fn printer<'a>(&'a self, source: &'a str) -> DiagnosticsPrinter<'a> {
        DiagnosticsPrinter::new(&self.messages, source)
    }

    pub fn extend(&mut self, other: Diagnostics) {
        self.messages.extend(other.messages);
    }
}

impl<'a> DiagnosticBuilder<'a> {
    pub fn related_to(mut self, msg: impl Into<String>, range: TextRange) -> Self {
        self.message.related.push(RelatedInfo::new(range, msg));
        self
    }

    pub fn fix(mut self, description: impl Into<String>, replacement: impl Into<String>) -> Self {
        self.message.fix = Some(Fix::new(replacement, description));
        self
    }

    pub fn emit(self) {
        self.diagnostics.messages.push(self.message);
    }
}
