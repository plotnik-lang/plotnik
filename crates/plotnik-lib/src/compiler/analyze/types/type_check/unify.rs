//! Unification logic for unlabeled alternation results.

use std::collections::{BTreeMap, BTreeSet};

use crate::compiler::analyze::types::capture::{InferredField, InferredFieldFlow};
use crate::compiler::analyze::types::type_analysis::{
    TypeAnalysisBuilder, TypeUnification, UnifyError,
};
use crate::compiler::analyze::types::type_shape::{
    ListMinimum, PatternFlow, PatternShape, RecordField, TypeShape,
};
use crate::compiler::diagnostics::span::Span;
use crate::compiler::parse::ast::Pattern;
use crate::core::Symbol;

pub(super) fn unify_alternative_flows(
    ctx: &mut TypeAnalysisBuilder,
    alternatives: impl IntoIterator<Item = (Option<Pattern>, PatternShape)>,
    fallback_span: Span,
) -> Result<Option<InferredFieldFlow>, UnifyError> {
    let mut merged = BTreeMap::new();
    let mut alternation_omissions = BTreeSet::new();
    let mut saw_alternative = false;

    for (pattern, shape) in alternatives {
        let fields = match shape.flow {
            PatternFlow::Fields(_) => {
                let pattern = pattern.expect("field-producing alternative has a body");
                let field_flow = shape
                    .field_flow
                    .as_ref()
                    .expect("inference retains field provenance");
                Some(InferredFieldFlow::forwarded(pattern, field_flow).fields)
            }
            PatternFlow::NoValue | PatternFlow::Value(_) => None,
        };

        match fields {
            Some(fields) => {
                if saw_alternative {
                    alternation_omissions
                        .extend(fields.keys().filter(|name| !merged.contains_key(*name)));
                    alternation_omissions
                        .extend(merged.keys().filter(|name| !fields.contains_key(name)));
                }
                merge_fields(ctx, &mut merged, fields, saw_alternative, fallback_span)?;
            }
            None if saw_alternative => alternation_omissions.extend(merged.keys().copied()),
            None => {}
        }
        saw_alternative = true;
    }

    if merged.is_empty() {
        return Ok(None);
    }

    let record = ctx.intern_record(
        merged
            .iter()
            .map(|(&name, field)| (name, field.info))
            .collect(),
    );
    Ok(Some(InferredFieldFlow::alternation(
        record,
        merged,
        alternation_omissions,
    )))
}

fn merge_fields(
    ctx: &mut TypeAnalysisBuilder,
    target: &mut BTreeMap<Symbol, InferredField>,
    source: BTreeMap<Symbol, InferredField>,
    relax_new: bool,
    fallback_span: Span,
) -> Result<(), UnifyError> {
    let mut missing_names = target.keys().copied().collect::<BTreeSet<_>>();

    for (name, source_field) in source {
        missing_names.remove(&name);
        let Some(target_field) = target.remove(&name) else {
            let field = if relax_new {
                relax_for_absence(ctx, source_field)
            } else {
                source_field
            };
            target.insert(name, field);
            continue;
        };

        let field = merge_field(ctx, name, target_field, source_field, fallback_span)?;
        target.insert(name, field);
    }

    for name in missing_names {
        if let Some(field) = target.remove(&name) {
            target.insert(name, relax_for_absence(ctx, field));
        }
    }
    Ok(())
}

fn merge_field(
    ctx: &mut TypeAnalysisBuilder,
    name: Symbol,
    mut left: InferredField,
    right: InferredField,
    fallback_span: Span,
) -> Result<InferredField, UnifyError> {
    let final_type = match ctx.unify_types(left.info.final_type, right.info.final_type) {
        TypeUnification::Unified(final_type) => final_type,
        outcome @ (TypeUnification::Distinct | TypeUnification::Pending) => {
            let mut name_spans = left
                .sources
                .iter()
                .chain(&right.sources)
                .map(|source| (source.name_span(), source.info().final_type))
                .collect::<Vec<_>>();
            name_spans.dedup();
            let mut producers = left.producers.clone();
            producers.extend(right.producers.iter().copied());
            let error = UnifyError::IncompatibleFieldTypes {
                field: name,
                left_type: left.info.final_type,
                right_type: right.info.final_type,
                name_spans,
                producers,
                fallback_span,
            };
            match outcome {
                TypeUnification::Pending => {
                    ctx.defer_unification(left.info.final_type, right.info.final_type, error)
                }
                TypeUnification::Distinct => return Err(error),
                TypeUnification::Unified(_) => unreachable!("matched a non-unified outcome"),
            }
        }
    };

    left.info = RecordField::new(final_type);
    left.producers.extend(right.producers);
    left.sources.extend(right.sources);
    Ok(left)
}

fn relax_for_absence(ctx: &mut TypeAnalysisBuilder, mut field: InferredField) -> InferredField {
    field.info = relax_record_field_for_absence(ctx, field.info);
    field
}

fn relax_record_field_for_absence(ctx: &mut TypeAnalysisBuilder, info: RecordField) -> RecordField {
    if let Some(TypeShape::List { element, .. }) = ctx.in_progress().type_shape(info.final_type) {
        let list = ctx.intern_type(TypeShape::List {
            element: *element,
            minimum: ListMinimum::Zero,
        });
        return RecordField::new(list);
    }
    RecordField::new(ctx.intern_option(info.final_type))
}
