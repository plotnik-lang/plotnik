use crate::ast::{Diagnostic, ErrorStage, RenderOptions, Severity, render_diagnostics};

use super::Query;

impl Query<'_> {
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.errors
    }

    /// Backwards compatibility alias for diagnostics()
    pub fn errors(&self) -> &[Diagnostic] {
        &self.errors
    }

    /// Query is valid if there are no errors (warnings are allowed).
    pub fn is_valid(&self) -> bool {
        !self.errors.iter().any(|d| d.is_error())
    }

    pub fn has_errors(&self) -> bool {
        self.errors.iter().any(|d| d.is_error())
    }

    pub fn has_warnings(&self) -> bool {
        self.errors.iter().any(|d| d.is_warning())
    }

    pub fn errors_only(&self) -> Vec<&Diagnostic> {
        self.errors.iter().filter(|d| d.is_error()).collect()
    }

    pub fn warnings_only(&self) -> Vec<&Diagnostic> {
        self.errors.iter().filter(|d| d.is_warning()).collect()
    }

    pub fn diagnostics_for_stage(&self, stage: ErrorStage) -> Vec<&Diagnostic> {
        self.errors.iter().filter(|d| d.stage == stage).collect()
    }

    /// Backwards compatibility alias
    pub fn errors_for_stage(&self, stage: ErrorStage) -> Vec<&Diagnostic> {
        self.diagnostics_for_stage(stage)
    }

    pub fn has_parse_errors(&self) -> bool {
        self.errors
            .iter()
            .any(|d| d.stage == ErrorStage::Parse && d.is_error())
    }

    pub fn has_validate_errors(&self) -> bool {
        self.errors
            .iter()
            .any(|d| d.stage == ErrorStage::Validate && d.is_error())
    }

    pub fn has_resolve_errors(&self) -> bool {
        self.errors
            .iter()
            .any(|d| d.stage == ErrorStage::Resolve && d.is_error())
    }

    pub fn has_escape_errors(&self) -> bool {
        self.errors
            .iter()
            .any(|d| d.stage == ErrorStage::Escape && d.is_error())
    }

    pub fn render_diagnostics(&self, options: RenderOptions) -> String {
        render_diagnostics(self.source, &self.errors, None, options)
    }

    pub fn error_count(&self) -> usize {
        self.errors.iter().filter(|d| d.is_error()).count()
    }

    pub fn warning_count(&self) -> usize {
        self.errors.iter().filter(|d| d.is_warning()).count()
    }

    pub fn filter_by_severity(&self, severity: Severity) -> Vec<&Diagnostic> {
        self.errors
            .iter()
            .filter(|d| d.severity == severity)
            .collect()
    }
}
