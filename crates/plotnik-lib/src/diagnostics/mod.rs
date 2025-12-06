mod message;
mod printer;

#[cfg(test)]
mod tests;

use rowan::TextRange;

pub use message::{DiagnosticKind, Severity};
pub use printer::DiagnosticsPrinter;

use message::{DiagnosticMessage, Fix, RelatedInfo};

#[derive(Debug, Clone, Default)]
pub struct Diagnostics {
    messages: Vec<DiagnosticMessage>,
}

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

    /// Create a diagnostic with the given kind and span.
    ///
    /// Uses the kind's default message. Call `.message()` on the builder to override.
    pub fn report(&mut self, kind: DiagnosticKind, range: TextRange) -> DiagnosticBuilder<'_> {
        DiagnosticBuilder {
            diagnostics: self,
            message: DiagnosticMessage::with_default_message(kind, range),
        }
    }

    /// Create an error diagnostic (legacy API, prefer `report()`).
    pub fn error(&mut self, msg: impl Into<String>, range: TextRange) -> DiagnosticBuilder<'_> {
        DiagnosticBuilder {
            diagnostics: self,
            message: DiagnosticMessage::new(DiagnosticKind::UnexpectedToken, range, msg),
        }
    }

    /// Create a warning diagnostic (legacy API, prefer `report()`).
    pub fn warning(&mut self, msg: impl Into<String>, range: TextRange) -> DiagnosticBuilder<'_> {
        DiagnosticBuilder {
            diagnostics: self,
            message: DiagnosticMessage::new(DiagnosticKind::UnexpectedToken, range, msg),
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

    /// Returns diagnostics with cascading errors suppressed.
    ///
    /// Suppression rule: when a higher-priority diagnostic's span contains
    /// a lower-priority diagnostic's span, the lower-priority one is suppressed.
    pub(crate) fn filtered(&self) -> Vec<DiagnosticMessage> {
        if self.messages.is_empty() {
            return Vec::new();
        }

        let mut suppressed = vec![false; self.messages.len()];

        // O(n²) but n is typically small (< 100 diagnostics)
        for (i, outer) in self.messages.iter().enumerate() {
            for (j, inner) in self.messages.iter().enumerate() {
                if i == j || suppressed[j] {
                    continue;
                }

                // Check if outer contains inner and has higher priority
                if span_contains(outer.range, inner.range) && outer.kind.suppresses(&inner.kind) {
                    suppressed[j] = true;
                }
            }
        }

        self.messages
            .iter()
            .enumerate()
            .filter(|(i, _)| !suppressed[*i])
            .map(|(_, m)| m.clone())
            .collect()
    }

    /// Raw access to all diagnostics (for debugging/testing).
    #[allow(dead_code)]
    pub(crate) fn raw(&self) -> &[DiagnosticMessage] {
        &self.messages
    }

    pub fn printer<'a>(&self, source: &'a str) -> DiagnosticsPrinter<'a> {
        DiagnosticsPrinter::new(self.messages.clone(), source)
    }

    /// Printer that uses filtered diagnostics (cascading errors suppressed).
    pub fn filtered_printer<'a>(&self, source: &'a str) -> DiagnosticsPrinter<'a> {
        DiagnosticsPrinter::new(self.filtered(), source)
    }

    pub fn render(&self, source: &str) -> String {
        self.printer(source).render()
    }

    pub fn render_colored(&self, source: &str, colored: bool) -> String {
        self.printer(source).colored(colored).render()
    }

    pub fn render_filtered(&self, source: &str) -> String {
        self.filtered_printer(source).render()
    }

    pub fn render_filtered_colored(&self, source: &str, colored: bool) -> String {
        self.filtered_printer(source).colored(colored).render()
    }

    pub fn extend(&mut self, other: Diagnostics) {
        self.messages.extend(other.messages);
    }
}

impl<'a> DiagnosticBuilder<'a> {
    /// Override the default message for this diagnostic kind.
    pub fn message(mut self, msg: impl Into<String>) -> Self {
        self.message.message = msg.into();
        self
    }

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

/// Check if outer span fully contains inner span.
fn span_contains(outer: TextRange, inner: TextRange) -> bool {
    outer.start() <= inner.start() && inner.end() <= outer.end()
}
