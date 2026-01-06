use rowan::TextRange;

use crate::diagnostics::DiagnosticKind;
use crate::parser::Parser;

use super::utils::{to_pascal_case, to_snake_case};

impl Parser<'_, '_> {
    /// Validate capture name follows plotnik convention (snake_case).
    pub(crate) fn validate_capture_name(&mut self, name: &str, span: TextRange) {
        if name.contains('.') {
            let suggested = name.replace(['.', '-'], "_");
            let suggested = to_snake_case(&suggested);
            self.error_with_fix(
                DiagnosticKind::CaptureNameHasDots,
                span,
                "captures become struct fields",
                format!("use `@{}`", suggested),
                suggested,
            );
            return;
        }

        if name.contains('-') {
            let suggested = name.replace('-', "_");
            let suggested = to_snake_case(&suggested);
            self.error_with_fix(
                DiagnosticKind::CaptureNameHasHyphens,
                span,
                "captures become struct fields",
                format!("use `@{}`", suggested),
                suggested,
            );
            return;
        }

        if name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
            let suggested = to_snake_case(name);
            self.error_with_fix(
                DiagnosticKind::CaptureNameUppercase,
                span,
                "captures become struct fields",
                format!("use `@{}`", suggested),
                suggested,
            );
        }
    }

    /// Validate definition name follows PascalCase convention.
    pub(crate) fn validate_def_name(&mut self, name: &str, span: TextRange) {
        if !name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
            let suggested = to_pascal_case(name);
            self.error_with_fix(
                DiagnosticKind::DefNameLowercase,
                span,
                "definitions map to types",
                format!("use `{}`", suggested),
                suggested,
            );
            return;
        }

        if name.contains('_') || name.contains('-') || name.contains('.') {
            let suggested = to_pascal_case(name);
            self.error_with_fix(
                DiagnosticKind::DefNameHasSeparators,
                span,
                "definitions map to types",
                format!("use `{}`", suggested),
                suggested,
            );
        }
    }

    /// Validate branch label follows PascalCase convention.
    pub(crate) fn validate_branch_label(&mut self, name: &str, span: TextRange) {
        if name.contains('_') || name.contains('-') || name.contains('.') {
            let suggested = to_pascal_case(name);
            self.error_with_fix(
                DiagnosticKind::BranchLabelHasSeparators,
                span,
                "branch labels map to enum variants",
                format!("use `{}:`", suggested),
                format!("{}:", suggested),
            );
        }
    }

    /// Validate field name follows snake_case convention.
    pub(crate) fn validate_field_name(&mut self, name: &str, span: TextRange) {
        if name.contains('.') {
            let suggested = name.replace(['.', '-'], "_");
            let suggested = to_snake_case(&suggested);
            self.error_with_fix(
                DiagnosticKind::FieldNameHasDots,
                span,
                "field names become struct fields",
                format!("use `{}:`", suggested),
                format!("{}:", suggested),
            );
            return;
        }

        if name.contains('-') {
            let suggested = name.replace('-', "_");
            let suggested = to_snake_case(&suggested);
            self.error_with_fix(
                DiagnosticKind::FieldNameHasHyphens,
                span,
                "field names become struct fields",
                format!("use `{}:`", suggested),
                format!("{}:", suggested),
            );
            return;
        }

        if name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
            let suggested = to_snake_case(name);
            self.error_with_fix(
                DiagnosticKind::FieldNameUppercase,
                span,
                "field names become struct fields",
                format!("use `{}:`", suggested),
                format!("{}:", suggested),
            );
        }
    }

    /// Validate type annotation name (PascalCase for user types, snake_case for primitives allowed).
    pub(crate) fn validate_type_name(&mut self, name: &str, span: TextRange) {
        if name.contains('.') || name.contains('-') {
            let suggested = to_pascal_case(name);
            self.error_with_fix(
                DiagnosticKind::TypeNameInvalidChars,
                span,
                "type annotations map to types",
                format!("use `::{}`", suggested),
                format!("::{}", suggested),
            );
        }
    }
}
