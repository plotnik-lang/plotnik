use rowan::TextRange;

use crate::compiler::diagnostics::diagnostics::DiagnosticKind;
use crate::compiler::parse::parser::Parser;

use super::utils::{starts_uppercase, to_pascal_case, to_snake_case};

impl Parser<'_, '_> {
    /// Capture names are strictly snake_case: they become Rust struct fields.
    pub(crate) fn validate_capture_name(&mut self, name: &str, span: TextRange) {
        if name.contains(['.', '-']) || name.chars().any(|c| c.is_ascii_uppercase()) {
            let suggested = to_snake_case(&name.replace(['.', '-'], "_"));
            self.error_with_fix(
                DiagnosticKind::CaptureNameInvalid,
                span,
                format!("use `@{suggested}`"),
                format!("@{suggested}"),
            );
        }
    }

    /// Definition names are PascalCase (they become types in the output).
    pub(crate) fn validate_def_name(&mut self, name: &str, span: TextRange) {
        if !starts_uppercase(name) || name.contains(['_', '-', '.']) {
            let suggested = to_pascal_case(name);
            self.error_with_fix(
                DiagnosticKind::DefNameInvalid,
                span,
                format!("use `{suggested}`"),
                suggested,
            );
        }
    }

    /// Branch labels are PascalCase (they become enum variants).
    /// Lowercase labels take a separate parse path; this only checks separators.
    pub(crate) fn validate_branch_label(&mut self, name: &str, span: TextRange) {
        if name.contains(['_', '-', '.']) {
            let suggested = to_pascal_case(name);
            self.error_with_fix(
                DiagnosticKind::BranchLabelInvalid,
                span,
                format!("use `{suggested}`"),
                suggested,
            );
        }
    }

    /// Field names are snake_case (tree-sitter grammar convention).
    pub(crate) fn validate_field_name(&mut self, name: &str, span: TextRange) {
        if name.contains(['.', '-']) || starts_uppercase(name) {
            let suggested = to_snake_case(&name.replace(['.', '-'], "_"));
            self.error_with_fix(
                DiagnosticKind::FieldNameInvalid,
                span,
                format!("use `{suggested}`"),
                suggested,
            );
        }
    }

    /// Type names must be PascalCase identifiers; never `.`, `-`, or lowercase.
    pub(crate) fn validate_type_name(&mut self, name: &str, span: TextRange) {
        if name.contains(['.', '-']) || !starts_uppercase(name) {
            let suggested = to_pascal_case(name);
            self.error_with_fix(
                DiagnosticKind::TypeNameInvalid,
                span,
                format!("use `::{suggested}`"),
                suggested,
            );
        }
    }
}
