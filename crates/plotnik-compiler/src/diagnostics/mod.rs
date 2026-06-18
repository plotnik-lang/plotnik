mod json;
mod message;
mod printer;

#[cfg(test)]
mod diagnostics_tests;

use rowan::TextRange;

pub use json::{
    Diagnostic as JsonDiagnostic, Fix as JsonFix, Position as JsonPosition,
    Related as JsonRelated, Span as JsonSpan,
};
pub use message::{DiagnosticKind, Severity};

use printer::DiagnosticsPrinter;

use message::{Diagnostic, Fix, Related};

pub use crate::query::{SourceId, SourceMap};

/// A location that knows which source it belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub source: SourceId,
    pub range: TextRange,
}

impl Span {
    pub fn new(source: SourceId, range: TextRange) -> Self {
        Self { source, range }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Diagnostics {
    messages: Vec<Diagnostic>,
}

#[must_use = "diagnostic not emitted, call .emit()"]
pub struct DiagnosticBuilder<'d> {
    diagnostics: &'d mut Diagnostics,
    message: Diagnostic,
}

impl Diagnostics {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
        }
    }

    /// Uses the kind's default message; call `.detail()` on the builder to override.
    pub fn report(
        &mut self,
        source: SourceId,
        kind: DiagnosticKind,
        range: TextRange,
    ) -> DiagnosticBuilder<'_> {
        DiagnosticBuilder {
            diagnostics: self,
            message: Diagnostic::new(source, kind, range),
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

    /// Returns diagnostics with cascading errors suppressed.
    ///
    /// Suppression rules:
    /// 1. Containment: when error A's suppression_range contains error B's display range,
    ///    and A has higher priority, suppress B (only for structural errors)
    /// 2. Same position: when spans start at the same position, root-cause errors suppress structural ones
    /// 3. Consequence errors (MissingDefName) suppressed when any other error exists
    /// 4. Adjacent: when error A ends exactly where error B starts, A suppresses B
    pub(crate) fn live(&self) -> Vec<Diagnostic> {
        if self.messages.is_empty() {
            return Vec::new();
        }

        let mut suppressed = vec![false; self.messages.len()];

        // Rule 3: Suppress consequence errors if any non-consequence error exists
        let has_root_diagnostic = self
            .messages
            .iter()
            .any(|m| !m.kind.is_cascade_consequence());
        if has_root_diagnostic {
            for (i, msg) in self.messages.iter().enumerate() {
                if msg.kind.is_cascade_consequence() {
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

                // A warning never outranks an error
                if a.is_warning() && b.is_error() {
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
    pub(crate) fn raw(&self) -> &[Diagnostic] {
        &self.messages
    }

    /// Cascading errors are suppressed; see [`Self::render_raw`] for raw output.
    pub fn render(&self, sources: &SourceMap) -> String {
        DiagnosticsPrinter::new(self.live(), sources).render()
    }

    /// Cascading errors are suppressed; `colored` controls ANSI escape codes.
    pub fn render_colored(&self, sources: &SourceMap, colored: bool) -> String {
        DiagnosticsPrinter::new(self.live(), sources)
            .colored(colored)
            .render()
    }

    /// Render every diagnostic including suppressed cascades; for debugging only.
    pub fn render_raw(&self, sources: &SourceMap) -> String {
        DiagnosticsPrinter::new(self.messages.clone(), sources).render()
    }

    /// Cascading errors are suppressed, same as `render`.
    pub fn to_wire(&self, sources: &SourceMap) -> Vec<JsonDiagnostic> {
        self.live()
            .iter()
            .map(|m| json::Diagnostic::from_diagnostic(m, sources))
            .collect()
    }

    pub fn render_json(&self, sources: &SourceMap) -> String {
        serde_json::to_string(&self.to_wire(sources)).expect("diagnostics serialize to JSON")
    }

    pub fn extend(&mut self, other: Diagnostics) {
        self.messages.extend(other.messages);
    }
}

impl<'d> DiagnosticBuilder<'d> {
    /// Override the default message; rendered using the kind's template.
    pub fn detail(mut self, msg: impl Into<String>) -> Self {
        let detail = msg.into();
        self.message.message = self.message.kind.render(Some(&detail));
        self
    }

    pub fn related_to(
        mut self,
        source: SourceId,
        range: TextRange,
        msg: impl Into<String>,
    ) -> Self {
        self.message
            .related
            .push(Related::new(source, range, msg));
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
        self.message.fix = Some(Fix::new(description, replacement));
        self
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.message.hints.push(hint.into());
        self
    }

    pub fn emit(mut self) {
        if let Some(default_hint) = self.message.kind.hint() {
            self.message.hints.insert(0, default_hint.to_string());
        }
        self.diagnostics.messages.push(self.message);
    }
}
