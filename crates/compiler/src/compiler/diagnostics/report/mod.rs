mod json;
mod message;
mod printer;

use rowan::TextRange;

pub use json::{
    Diagnostic as JsonDiagnostic, Fix as JsonFix, Position as JsonPosition, Related as JsonRelated,
    Span as JsonSpan,
};
pub use message::{DiagnosticKind, Severity};

use printer::DiagnosticsPrinter;

use message::{Diagnostic, Fix, Related};

pub use crate::compiler::diagnostics::source::{SourceId, SourceMap};
pub use crate::compiler::diagnostics::span::Span;

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
    pub fn report(&mut self, kind: DiagnosticKind, span: Span) -> DiagnosticBuilder<'_> {
        DiagnosticBuilder {
            diagnostics: self,
            message: Diagnostic::new(kind, span),
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

    pub(crate) fn live(&self) -> Vec<&Diagnostic> {
        if self.messages.is_empty() {
            return Vec::new();
        }

        let mut suppressed = vec![false; self.messages.len()];

        for (i, msg) in self.messages.iter().enumerate() {
            if !msg.kind.is_cascade_consequence() {
                continue;
            }
            let has_root_in_source = self
                .messages
                .iter()
                .any(|m| m.span.source == msg.span.source && !m.kind.is_cascade_consequence());
            if has_root_in_source {
                suppressed[i] = true;
            }
        }

        for (i, a) in self.messages.iter().enumerate() {
            for (j, b) in self.messages.iter().enumerate() {
                if i == j || suppressed[i] || suppressed[j] {
                    continue;
                }

                if a.span.source != b.span.source {
                    continue;
                }

                if a.is_warning() && b.is_error() {
                    continue;
                }

                let contains = a.suppression_range.start() <= b.span.range.start()
                    && b.span.range.end() <= a.suppression_range.end();
                if contains && a.kind.is_structural_error() && a.kind.suppresses(&b.kind) {
                    suppressed[j] = true;
                    continue;
                }

                if a.span.range.start() == b.span.range.start() {
                    if a.kind.is_root_cause_error() && b.kind.is_structural_error() {
                        suppressed[j] = true;
                        continue;
                    }
                    if a.kind.suppresses(&b.kind) {
                        suppressed[j] = true;
                        continue;
                    }
                }

                if a.span.range.end() == b.span.range.start() {
                    suppressed[j] = true;
                }
            }
        }

        let mut result: Vec<_> = self
            .messages
            .iter()
            .enumerate()
            .filter(|(i, _)| !suppressed[*i])
            .map(|(_, m)| m)
            .collect();
        result.sort_by_key(|m| (m.span.source, m.span.range.start()));
        result
    }

    /// The kind of every diagnostic, including suppressed cascades.
    pub fn kinds(&self) -> impl Iterator<Item = DiagnosticKind> + '_ {
        self.messages.iter().map(|d| d.kind)
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
        DiagnosticsPrinter::new(self.messages.iter().collect(), sources).render()
    }

    /// Cascading errors are suppressed, same as `render`.
    pub fn to_wire(&self, sources: &SourceMap) -> Vec<JsonDiagnostic> {
        self.live()
            .iter()
            .map(|&m| json::Diagnostic::from_diagnostic(m, sources))
            .collect()
    }

    pub fn extend(&mut self, other: Diagnostics) {
        self.messages.extend(other.messages);
    }

    pub(crate) fn add_grouping_hint(&mut self, source: SourceId) {
        if let Some(diagnostic) = self.messages.iter_mut().find(|message| {
            message.span.source == source
                && matches!(
                    message.kind,
                    DiagnosticKind::UnknownNodeKind | DiagnosticKind::UnknownGrammarField
                )
                && message.fix.is_none()
                && message.hints.is_empty()
        }) {
            diagnostic.hints.push(
                "if this file is for another language, set its first-line language tag".to_owned(),
            );
        }
    }
}

impl<'d> DiagnosticBuilder<'d> {
    /// Override the default message; rendered using the kind's template.
    pub fn detail(mut self, msg: impl Into<String>) -> Self {
        let detail = msg.into();
        self.message.message = self.message.kind.render(Some(&detail));
        self
    }

    pub fn related_to(mut self, span: Span, msg: impl Into<String>) -> Self {
        self.message.related.push(Related::new(span, msg));
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
