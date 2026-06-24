use rowan::TextRange;

use crate::compiler::diagnostics::diagnostics::DiagnosticKind;
use crate::compiler::diagnostics::source::SourceId;
use crate::compiler::diagnostics::span::Span;
use crate::compiler::parse::ast::{FieldPattern, Pattern, QuantifiedPattern};

use super::super::types::{OutputFlow, PatternResult, TypeId};
use super::super::unify::UnifyError;
use super::InferVisitor;

impl InferVisitor<'_, '_> {
    pub(super) fn report_field_arity_error(&mut self, field: &FieldPattern, value: &Pattern) {
        let field_name = field
            .name()
            .map(|t| t.text().to_string())
            .unwrap_or_else(|| "field".to_string());

        let related = self.referenced_definition_range(value);

        let mut builder = self
            .report(DiagnosticKind::FieldSequenceValue, value.text_range())
            .detail(field_name);
        if let Some((src, range)) = related {
            builder = builder.related_to(Span::new(src, range), "defined here");
        }

        builder.emit();
    }

    fn referenced_definition_range(&self, value: &Pattern) -> Option<(SourceId, TextRange)> {
        let Pattern::DefRef(r) = value else {
            return None;
        };
        let name = r.name()?;
        let (source, body) = self.ctx.symbol_table.definition(name.text())?;
        Some((source, body.text_range()))
    }

    /// Strict-dimensionality check 1: a multi-element pattern (`Arity::Many`)
    /// without captures can't become a scalar array. Applies even under a row
    /// capture — you can't meaningfully capture multiple nodes per iteration as
    /// a scalar. Returns `true` when it reports, signalling the caller to skip
    /// the internal-capture check (the original short-circuit).
    pub(super) fn check_multi_element_scalar(
        &mut self,
        quant: &QuantifiedPattern,
        inner_info: &PatternResult,
    ) -> bool {
        let is_multi_element_scalar =
            inner_info.arity == super::super::types::Arity::Many && inner_info.flow.is_void();
        if !is_multi_element_scalar {
            return false;
        }

        let op = self.quantifier_operator(quant);
        self.report(
            DiagnosticKind::MultiElementScalarCapture,
            quant.text_range(),
        )
        .detail(format!(
            "sequence with `{}` matches multiple nodes but has no internal captures",
            op
        ))
        .emit();
        true
    }

    /// Strict-dimensionality check 2: internal captures require a row capture on
    /// the quantifier. Skipped when inference runs in row-capture mode.
    pub(super) fn check_internal_capture_dimensionality(
        &mut self,
        quant: &QuantifiedPattern,
        inner_info: &PatternResult,
    ) {
        let OutputFlow::Fields(type_id) = &inner_info.flow else {
            return;
        };

        let type_ctx = self.ctx.type_ctx.in_progress();
        let fields = type_ctx.expect_struct_fields(*type_id);
        if fields.is_empty() {
            return;
        }

        let capture_names: Vec<_> = fields
            .keys()
            .map(|s| format!("`@{}`", self.ctx.interner.resolve(*s)))
            .collect();
        let captures_str = capture_names.join(", ");

        let op = self.quantifier_operator(quant);
        self.report(
            DiagnosticKind::StrictDimensionalityViolation,
            quant.text_range(),
        )
        .detail(format!(
            "quantifier `{}` contains captures ({}) but has no struct capture",
            op, captures_str
        ))
        .hint(format!("add a struct capture: `{{...}}{} @name`", op))
        .emit();
    }

    pub(super) fn report_ambiguous_outputs(
        &mut self,
        parent_range: TextRange,
        outputs: &[(TextRange, TypeId)],
    ) {
        let source = self.source;
        let mut builder = self
            .report(DiagnosticKind::AmbiguousUncapturedOutputs, parent_range)
            .detail(format!(
                "{} expressions here produce a value but none is captured",
                outputs.len()
            ));
        for (range, _) in outputs {
            builder = builder.related_to(Span::new(source, *range), "produces a value");
        }
        builder.emit();
    }

    pub(super) fn report_uncaptured_output_with_captures(
        &mut self,
        outputs: &[(TextRange, TypeId)],
    ) {
        for (range, _) in outputs {
            self.report(DiagnosticKind::UncapturedOutputWithCaptures, *range)
                .emit();
        }
    }

    pub(super) fn report_unify_error(&mut self, range: TextRange, err: &UnifyError) {
        let (kind, msg, hint) = match err {
            UnifyError::ScalarInUnion => (
                DiagnosticKind::IncompatibleTypes,
                "a branch produces a value but this is a union alternation".to_string(),
                Some("give every branch a branch label for an enum, e.g. `[A: ... B: ...]`"),
            ),
            UnifyError::IncompatibleTypes { field } => (
                DiagnosticKind::IncompatibleCaptureTypes,
                self.ctx.interner.resolve(*field).to_string(),
                Some("make every branch produce the same type, or label the branches for an enum"),
            ),
        };

        let mut builder = self.report(kind, range).detail(msg);
        if let Some(h) = hint {
            builder = builder.hint(h);
        }
        builder.emit();
    }

    fn quantifier_operator(&self, quant: &QuantifiedPattern) -> String {
        quant
            .operator()
            .map(|t| t.text().to_string())
            .unwrap_or_else(|| "*".to_string())
    }
}
