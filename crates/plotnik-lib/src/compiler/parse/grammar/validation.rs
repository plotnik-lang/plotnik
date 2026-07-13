use rowan::TextRange;

use crate::compiler::diagnostics::report::DiagnosticKind;
use crate::compiler::parse::Parser;

use super::utils::{starts_uppercase, to_pascal_case, to_snake_case};

pub(crate) struct Ident<'q> {
    text: &'q str,
    span: TextRange,
}

impl<'q> Ident<'q> {
    pub(crate) fn new(text: &'q str, span: TextRange) -> Self {
        Self { text, span }
    }

    pub(crate) fn text(&self) -> &'q str {
        self.text
    }

    pub(crate) fn span(&self) -> TextRange {
        self.span
    }
}

impl<'q> Parser<'q, '_> {
    pub(crate) fn bump_ident(&mut self) -> Ident<'q> {
        self.assert_current(crate::compiler::parse::cst::SyntaxKind::Id);
        let ident = Ident::new(self.current_text(), self.current_span());
        self.bump();
        ident
    }

    /// Capture names are strictly snake_case: they become result fields.
    pub(crate) fn validate_capture_name(&mut self, ident: Ident<'_>) {
        let name = ident.text();
        if name.contains(['.', '-']) || name.chars().any(|c| c.is_ascii_uppercase()) {
            let suggested = to_snake_case(&name.replace(['.', '-'], "_"));
            if let Some(report) = self.report_at(DiagnosticKind::CaptureNameInvalid, ident.span()) {
                report
                    .fix(format!("use `@{suggested}`"), format!("@{suggested}"))
                    .emit();
            }
        }
    }

    /// Definition names are PascalCase (they become types in the output).
    pub(crate) fn validate_def_name(&mut self, ident: Ident<'_>) {
        let name = ident.text();
        if !starts_uppercase(name) || name.contains(['_', '-', '.']) {
            let suggested = to_pascal_case(name);
            if let Some(report) = self.report_at(DiagnosticKind::DefNameInvalid, ident.span()) {
                report.fix(format!("use `{suggested}`"), suggested).emit();
            }
        }
    }

    /// Alternative labels are PascalCase (they become variant cases).
    /// Lowercase labels take a separate parse path; this only checks separators.
    pub(crate) fn validate_alternative_label(&mut self, ident: Ident<'_>) {
        let name = ident.text();
        if name.contains(['_', '-', '.']) {
            let suggested = to_pascal_case(name);
            if let Some(report) =
                self.report_at(DiagnosticKind::AlternativeLabelInvalid, ident.span())
            {
                report.fix(format!("use `{suggested}`"), suggested).emit();
            }
        }
    }

    /// Field names are snake_case (tree-sitter grammar convention).
    pub(crate) fn validate_field_name(&mut self, ident: Ident<'_>) {
        let name = ident.text();
        if name.contains(['.', '-']) || starts_uppercase(name) {
            let suggested = to_snake_case(&name.replace(['.', '-'], "_"));
            if let Some(report) = self.report_at(DiagnosticKind::FieldNameInvalid, ident.span()) {
                report.fix(format!("use `{suggested}`"), suggested).emit();
            }
        }
    }

    /// Custom capture types are PascalCase. Lowercase identifiers are complete
    /// syntax here so analysis can recognize built-ins and diagnose unknown ones.
    pub(crate) fn validate_capture_type_name(&mut self, ident: Ident<'_>) {
        let name = ident.text();
        let malformed = name.contains(['.', '-']) || starts_uppercase(name) && name.contains('_');
        if !malformed {
            return;
        }

        let report = self.report_at(DiagnosticKind::CaptureTypeNameInvalid, ident.span());
        if starts_uppercase(name) {
            let suggested = to_pascal_case(name);
            if let Some(report) = report {
                report.fix(format!("use `::{suggested}`"), suggested).emit();
            }
            return;
        }

        if let Some(report) = report {
            report.emit();
        }
    }
}
