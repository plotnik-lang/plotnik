use rowan::TextRange;

use crate::compiler::diagnostics::diagnostics::DiagnosticKind;
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

    /// Capture names are strictly snake_case: they become Rust struct fields.
    pub(crate) fn validate_capture_name(&mut self, ident: Ident<'_>) {
        let name = ident.text();
        if name.contains(['.', '-']) || name.chars().any(|c| c.is_ascii_uppercase()) {
            let suggested = to_snake_case(&name.replace(['.', '-'], "_"));
            self.error_with_fix(
                DiagnosticKind::CaptureNameInvalid,
                ident.span(),
                format!("use `@{suggested}`"),
                format!("@{suggested}"),
            );
        }
    }

    /// Definition names are PascalCase (they become types in the output).
    pub(crate) fn validate_def_name(&mut self, ident: Ident<'_>) {
        let name = ident.text();
        if !starts_uppercase(name) || name.contains(['_', '-', '.']) {
            let suggested = to_pascal_case(name);
            self.error_with_fix(
                DiagnosticKind::DefNameInvalid,
                ident.span(),
                format!("use `{suggested}`"),
                suggested,
            );
        }
    }

    /// Branch labels are PascalCase (they become enum variants).
    /// Lowercase labels take a separate parse path; this only checks separators.
    pub(crate) fn validate_branch_label(&mut self, ident: Ident<'_>) {
        let name = ident.text();
        if name.contains(['_', '-', '.']) {
            let suggested = to_pascal_case(name);
            self.error_with_fix(
                DiagnosticKind::BranchLabelInvalid,
                ident.span(),
                format!("use `{suggested}`"),
                suggested,
            );
        }
    }

    /// Field names are snake_case (tree-sitter grammar convention).
    pub(crate) fn validate_field_name(&mut self, ident: Ident<'_>) {
        let name = ident.text();
        if name.contains(['.', '-']) || starts_uppercase(name) {
            let suggested = to_snake_case(&name.replace(['.', '-'], "_"));
            self.error_with_fix(
                DiagnosticKind::FieldNameInvalid,
                ident.span(),
                format!("use `{suggested}`"),
                suggested,
            );
        }
    }

    /// Type names must be PascalCase identifiers; never `.`, `-`, or lowercase.
    pub(crate) fn validate_type_name(&mut self, ident: Ident<'_>) {
        let name = ident.text();
        if name.contains(['.', '-']) || !starts_uppercase(name) {
            let suggested = to_pascal_case(name);
            self.error_with_fix(
                DiagnosticKind::TypeNameInvalid,
                ident.span(),
                format!("use `::{suggested}`"),
                suggested,
            );
        }
    }
}
