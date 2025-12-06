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
    /// Suppression rules:
    /// 1. Containment: when error A's suppression_range contains error B's display range,
    ///    and A has higher priority, suppress B (only for structural errors)
    /// 2. Same position: when spans start at the same position, root-cause errors suppress structural ones
    /// 3. Consequence errors (UnnamedDefNotLast) suppressed when any other error exists
    /// 4. Adjacent: when error A ends exactly where error B starts, A suppresses B
    pub(crate) fn filtered(&self) -> Vec<DiagnosticMessage> {
        if self.messages.is_empty() {
            return Vec::new();
        }

        let mut suppressed = vec![false; self.messages.len()];

        // Rule 3: Suppress consequence errors if any non-consequence error exists
        let has_primary_error = self.messages.iter().any(|m| !m.kind.is_consequence_error());
        if has_primary_error {
            for (i, msg) in self.messages.iter().enumerate() {
                if msg.kind.is_consequence_error() {
                    suppressed[i] = true;
                }
            }
        }

        // O(n²) but n is typically small (< 100 diagnostics)
        for (i, a) in self.messages.iter().enumerate() {
            for (j, b) in self.messages.iter().enumerate() {
                if i == j || suppressed[i] || suppressed[j] {
                    continue;
                }

                // Rule 1: Structural error containment
                // Only unclosed delimiters can suppress distant errors, because they cause
                // cascading parse failures throughout the tree
                let contains = a.suppression_range.start() <= b.range.start()
                    && b.range.end() <= a.suppression_range.end();
                if contains && a.kind.is_structural_error() && a.kind.suppresses(&b.kind) {
                    suppressed[j] = true;
                    continue;
                }

                // Rule 2: Same start position
                if a.range.start() == b.range.start() {
                    // Root cause errors (Expected*) suppress structural errors (Unclosed*)
                    // even though structural errors have higher enum priority. This is because
                    // ExpectedExpression is the actual mistake; UnclosedTree is a consequence.
                    if a.kind.is_root_cause_error() && b.kind.is_structural_error() {
                        suppressed[j] = true;
                        continue;
                    }
                    if a.kind.suppresses(&b.kind) {
                        suppressed[j] = true;
                        continue;
                    }
                }

                // Rule 4: Adjacent position - when A ends exactly where B starts,
                // B is likely a consequence of A (e.g., `@x` where `@` is unexpected
                // and `x` would be reported as bare identifier).
                // Priority doesn't matter here - position determines causality.
                if a.range.end() == b.range.start() {
                    suppressed[j] = true;
                }
            }
        }

        let mut result: Vec<_> = self
            .messages
            .iter()
            .enumerate()
            .filter(|(i, _)| !suppressed[*i])
            .map(|(_, m)| m.clone())
            .collect();
        result.sort_by_key(|m| m.range.start());
        result
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
    /// Provide custom detail for this diagnostic, rendered using the kind's template.
    pub fn message(mut self, msg: impl Into<String>) -> Self {
        let detail = msg.into();
        self.message.message = self.message.kind.message(Some(&detail));
        self
    }

    pub fn related_to(mut self, msg: impl Into<String>, range: TextRange) -> Self {
        self.message.related.push(RelatedInfo::new(range, msg));
        self
    }

    /// Set the suppression range for this diagnostic.
    ///
    /// The suppression range is used to suppress cascading errors. Errors whose
    /// display range falls within another error's suppression range may be
    /// suppressed if the containing error has higher priority.
    ///
    /// Typically set to the parent context span (e.g., enclosing tree).
    pub fn suppression_range(mut self, range: TextRange) -> Self {
        self.message.suppression_range = range;
        self
    }

    pub fn fix(mut self, description: impl Into<String>, replacement: impl Into<String>) -> Self {
        self.message.fix = Some(Fix::new(replacement, description));
        self
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.message.hints.push(hint.into());
        self
    }

    pub fn emit(self) {
        self.diagnostics.messages.push(self.message);
    }
}
