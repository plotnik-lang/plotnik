use rowan::TextRange;

use crate::compiler::diagnostics::diagnostics::DiagnosticKind;
use crate::compiler::diagnostics::source::SourceId;
use crate::compiler::parse::ast::{FieldPattern, Pattern, QuantifiedPattern};

use super::InferVisitor;
use super::super::types::{OutputFlow, PatternResult, TypeId};
use super::super::unify::UnifyError;

impl InferVisitor<'_, '_> {
    pub(super) fn report_field_arity_error(
        &mut self,
        source: SourceId,
        field: &FieldPattern,
        value: &Pattern,
    ) {
        let field_name = field
            .name()
            .map(|t| t.text().to_string())
            .unwrap_or_else(|| "field".to_string());

        let mut builder = self.ctx.diag.report(
            source,
            DiagnosticKind::FieldSequenceValue,
            value.text_range(),
        );
        builder = builder.detail(field_name);

        if let Pattern::Ref(r) = value
            && let Some(name_tok) = r.name()
        {
            let name = name_tok.text();
            if let Some((src, body)) = self.ctx.symbol_table.definition(name) {
                builder = builder.related_to(src, body.text_range(), "defined here");
            }
        }

        builder.emit();
    }

    /// Strict-dimensionality check 1: a multi-element pattern (`Arity::Many`)
    /// without captures can't become a scalar array. Applies even under a row
    /// capture — you can't meaningfully capture multiple nodes per iteration as
    /// a scalar. Returns `true` when it reports, signalling the caller to skip
    /// the internal-capture check (the original short-circuit).
    pub(super) fn check_multi_element_scalar(
        &mut self,
        source: SourceId,
        quant: &QuantifiedPattern,
        inner_info: &PatternResult,
    ) -> bool {
        if !(inner_info.arity == super::super::types::Arity::Many && inner_info.flow.is_void()) {
            return false;
        }

        let op = self.quantifier_operator(quant);
        self.ctx
            .diag
            .report(
                source,
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
    /// the quantifier. Skipped when the quantifier already sits under a row
    /// capture (see `infer_quantified_pattern_as_row`).
    pub(super) fn check_internal_capture_dimensionality(
        &mut self,
        source: SourceId,
        quant: &QuantifiedPattern,
        inner_info: &PatternResult,
    ) {
        let OutputFlow::Fields(type_id) = &inner_info.flow else {
            return;
        };

        let fields = self.ctx.type_ctx.expect_struct_fields(*type_id);
        if fields.is_empty() {
            return;
        }

        let capture_names: Vec<_> = fields
            .keys()
            .map(|s| format!("`@{}`", self.ctx.interner.resolve(*s)))
            .collect();
        let captures_str = capture_names.join(", ");

        let op = self.quantifier_operator(quant);
        self.ctx
            .diag
            .report(
                source,
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
        source: SourceId,
        parent_range: TextRange,
        outputs: &[(TextRange, TypeId)],
    ) {
        let mut builder = self
            .ctx
            .diag
            .report(
                source,
                DiagnosticKind::AmbiguousUncapturedOutputs,
                parent_range,
            )
            .detail(format!(
                "{} expressions here produce a value but none is captured",
                outputs.len()
            ));
        for (range, _) in outputs {
            builder = builder.related_to(source, *range, "produces a value");
        }
        builder.emit();
    }

    pub(super) fn report_uncaptured_output_with_captures(
        &mut self,
        source: SourceId,
        outputs: &[(TextRange, TypeId)],
    ) {
        for (range, _) in outputs {
            self.ctx
                .diag
                .report(source, DiagnosticKind::UncapturedOutputWithCaptures, *range)
                .emit();
        }
    }

    pub(super) fn report_unify_error(
        &mut self,
        source: SourceId,
        range: TextRange,
        err: &UnifyError,
    ) {
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

        let mut builder = self.ctx.diag.report(source, kind, range).detail(msg);
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
