use crate::diagnostics::DiagnosticMessage;

use super::Query;

impl Query<'_> {
    pub fn diagnostics(&self) -> &[DiagnosticMessage] {
        self.diagnostics.as_slice()
    }

    /// Query is valid if there are no error-severity diagnostics (warnings are allowed).
    pub fn is_valid(&self) -> bool {
        !self.diagnostics.iter().any(|d| d.is_error())
    }

    pub fn diagnostics_printer(
        &self,
    ) -> crate::diagnostics::DiagnosticsPrinter<'_, impl Iterator<Item = &DiagnosticMessage> + Clone>
    {
        self.diagnostics.printer().source(self.source)
    }
}
