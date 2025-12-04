use crate::diagnostics::{DiagnosticMessage, DiagnosticStage, DiagnosticsPrinter};

use super::Query;

impl Query<'_> {
    pub fn diagnostics(&self) -> &[DiagnosticMessage] {
        self.diagnostics.as_slice()
    }

    /// Query is valid if there are no error-severity diagnostics (warnings are allowed).
    pub fn is_valid(&self) -> bool {
        !self.diagnostics.iter().any(|d| d.is_error())
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.is_error())
    }

    pub fn has_warnings(&self) -> bool {
        self.diagnostics.iter().any(|d| d.is_warning())
    }

    pub fn errors_only(&self) -> Vec<&DiagnosticMessage> {
        self.diagnostics.iter().filter(|d| d.is_error()).collect()
    }

    pub fn warnings_only(&self) -> Vec<&DiagnosticMessage> {
        self.diagnostics.iter().filter(|d| d.is_warning()).collect()
    }

    pub fn diagnostics_for_stage(&self, stage: DiagnosticStage) -> Vec<&DiagnosticMessage> {
        self.diagnostics
            .iter()
            .filter(|d| d.stage == stage)
            .collect()
    }

    pub fn has_parse_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.stage == DiagnosticStage::Parse && d.is_error())
    }

    pub fn has_validate_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.stage == DiagnosticStage::Validate && d.is_error())
    }

    pub fn has_resolve_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.stage == DiagnosticStage::Resolve && d.is_error())
    }

    pub fn has_escape_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.stage == DiagnosticStage::Escape && d.is_error())
    }

    pub fn diagnostics_printer(&self) -> DiagnosticsPrinter<'_, '_> {
        self.diagnostics.printer().source(self.source)
    }

    pub fn render_diagnostics_colored(&self, colored: bool) -> String {
        self.diagnostics_printer().colored(colored).render()
    }

    pub fn error_count(&self) -> usize {
        self.diagnostics.iter().filter(|d| d.is_error()).count()
    }
}
