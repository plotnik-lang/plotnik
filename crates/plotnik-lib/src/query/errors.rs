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

#[cfg(test)]
mod tests {
    use crate::ast::{ErrorStage, RenderOptions, Severity};
    use crate::query::Query;

    #[test]
    fn diagnostics_alias() {
        let q = Query::new("(valid)").unwrap();
        assert_eq!(q.diagnostics().len(), q.errors().len());
    }

    #[test]
    fn error_stage_filtering() {
        let q = Query::new("(unclosed").unwrap();
        assert!(q.has_parse_errors());
        assert!(!q.has_resolve_errors());
        assert!(!q.has_escape_errors());
        assert_eq!(q.errors_for_stage(ErrorStage::Parse).len(), 1);

        let q = Query::new("(call (Undefined))").unwrap();
        assert!(!q.has_parse_errors());
        assert!(q.has_resolve_errors());
        assert!(!q.has_escape_errors());
        assert_eq!(q.errors_for_stage(ErrorStage::Resolve).len(), 1);

        let q = Query::new("[A: (a) (b)]").unwrap();
        assert!(!q.has_parse_errors());
        assert!(q.has_validate_errors());
        assert!(!q.has_resolve_errors());
        assert!(!q.has_escape_errors());
        assert_eq!(q.errors_for_stage(ErrorStage::Validate).len(), 1);

        let q = Query::new("Expr = (call (Expr))").unwrap();
        assert!(!q.has_parse_errors());
        assert!(!q.has_validate_errors());
        assert!(!q.has_resolve_errors());
        assert!(q.has_escape_errors());
        assert_eq!(q.errors_for_stage(ErrorStage::Escape).len(), 1);

        let q = Query::new("Expr = (call (Expr)) (unclosed").unwrap();
        assert!(q.has_parse_errors());
        assert!(!q.has_resolve_errors());
        assert!(q.has_escape_errors());
    }

    #[test]
    fn is_valid_ignores_warnings() {
        // Currently all diagnostics are errors, so this just tests the basic case
        let q = Query::new("(valid)").unwrap();
        assert!(q.is_valid());
        assert!(!q.has_errors());
        assert!(!q.has_warnings());
        assert_eq!(q.error_count(), 0);
        assert_eq!(q.warning_count(), 0);
    }

    #[test]
    fn error_and_warning_counts() {
        let q = Query::new("(unclosed").unwrap();
        assert!(q.has_errors());
        assert!(!q.has_warnings());
        assert_eq!(q.error_count(), 1);
        assert_eq!(q.warning_count(), 0);
    }

    #[test]
    fn errors_only_and_warnings_only() {
        let q = Query::new("(unclosed").unwrap();
        let errors = q.errors_only();
        let warnings = q.warnings_only();
        assert_eq!(errors.len(), 1);
        assert!(warnings.is_empty());
    }

    #[test]
    fn render_diagnostics_method() {
        let q = Query::new("(unclosed").unwrap();
        let rendered = q.render_diagnostics(RenderOptions::plain());
        insta::assert_snapshot!(rendered, @r"
        error: expected closing ')' for tree
          |
        1 | (unclosed
          |          ^ expected closing ')' for tree
        ");
    }

    #[test]
    fn filter_by_severity() {
        let q = Query::new("(unclosed").unwrap();
        let errors = q.filter_by_severity(Severity::Error);
        let warnings = q.filter_by_severity(Severity::Warning);
        assert_eq!(errors.len(), 1);
        assert!(warnings.is_empty());
    }
}
