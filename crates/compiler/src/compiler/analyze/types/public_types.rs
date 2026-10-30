use std::collections::{BTreeMap, BTreeSet, HashSet};

use super::type_analysis::TypeAnalysisBuilder;
use super::type_shape::{DefinitionOutput, TypeId, TypeShape};
use crate::compiler::analyze::refs::DefinitionGraph;
use crate::compiler::diagnostics::{SourceId, Span};
use crate::core::Symbol;

#[derive(Clone, Debug)]
struct PublicType {
    output: DefinitionOutput,
    owner: Result<Span, Vec<Span>>,
    scope: Span,
}

#[derive(Clone, Debug, Default)]
pub(super) struct PublicTypes {
    items: BTreeMap<Symbol, PublicType>,
    uses: Vec<(Span, SourceId)>,
}

impl PublicTypes {
    pub(super) fn analyze(
        builder: &TypeAnalysisBuilder,
        definitions: &DefinitionGraph,
    ) -> (Self, HashSet<Span>) {
        let types = &builder.analysis;
        let mut public = Self::default();
        let mut pending = Vec::new();
        for id in definitions.ids_in_def_id_order() {
            let definition = definitions.definition(id);
            public.items.insert(
                definition.name(),
                PublicType {
                    output: types.expect_def_output(id),
                    owner: Ok(definition.span()),
                    scope: definition.span(),
                },
            );
            pending.push(definition.name());
        }

        let mut candidates = BTreeMap::<Symbol, Vec<(TypeId, Span, Span)>>::new();
        for occurrence in builder.custom_capture_types() {
            if types.public_name(occurrence.type_id) == Some(occurrence.name) {
                candidates.entry(occurrence.name).or_default().push((
                    occurrence.type_id,
                    occurrence.span,
                    occurrence.scope,
                ));
            }
        }
        let mut inferred = BTreeMap::<Symbol, Vec<(TypeId, Span, Span)>>::new();
        for &(ty, span) in &builder.type_origins {
            if let Some(name) = types.public_name(ty) {
                inferred.entry(name).or_default().push((ty, span, span));
            }
        }
        let mut ownership_annotations = HashSet::new();
        for (_, name) in types.iter_named_types() {
            if public.items.contains_key(&name) {
                continue;
            }

            let inferred = inferred.remove(&name);
            let mut sites = if let Some(sites) = candidates.remove(&name) {
                let sources = inferred
                    .iter()
                    .flatten()
                    .map(|&(_, span, _)| span.source)
                    .collect::<BTreeSet<_>>();
                for &(_, span, _) in &sites {
                    if sources.len() > 1
                        || sources.first().is_some_and(|&source| source != span.source)
                    {
                        ownership_annotations.insert(span);
                    }
                }
                sites
            } else {
                inferred.expect("an inferred public type retains its source origins")
            };
            sites.sort_by_key(|&(_, span, _)| (span.source, span.range.start(), span.range.end()));
            sites.dedup();
            let &(representative, first, scope) = sites
                .first()
                .expect("a live public type retains its declaration or inferred origins");
            let owner = if sites
                .iter()
                .all(|&(_, span, _)| span.source == first.source)
            {
                Ok(first)
            } else {
                Err(sites.iter().map(|&(_, span, _)| span).collect())
            };
            let body = match types.expect_type_shape(representative) {
                TypeShape::Ref(declaration) => types
                    .declaration_body(*declaration)
                    .expect("named capture declaration has a body"),
                _ => representative,
            };
            public.items.insert(
                name,
                PublicType {
                    output: DefinitionOutput::Value(body),
                    owner,
                    scope,
                },
            );
        }

        let mut visited = HashSet::new();
        while let Some(name) = pending.pop() {
            if !visited.insert(name) {
                continue;
            }
            let item = &public.items[&name];
            let (Some(body), Ok(_)) = (item.output.value(), item.owner.as_ref()) else {
                continue;
            };
            public.collect_uses(builder, body, item.scope, &mut pending);
        }
        public.uses.sort_by_key(|(span, owner)| {
            (span.source, span.range.start(), span.range.end(), *owner)
        });
        public.uses.dedup();
        (public, ownership_annotations)
    }

    pub(super) fn output(&self, name: Symbol) -> DefinitionOutput {
        self.items
            .get(&name)
            .expect("analysis assigns a representative to every public type")
            .output
    }

    pub(super) fn source(&self, name: Symbol) -> Result<SourceId, &[Span]> {
        self.items
            .get(&name)
            .expect("analysis assigns ownership to every public type")
            .owner
            .as_ref()
            .map(|span| span.source)
            .map_err(Vec::as_slice)
    }

    pub(super) fn uses(&self) -> &[(Span, SourceId)] {
        &self.uses
    }

    fn collect_uses(
        &mut self,
        builder: &TypeAnalysisBuilder,
        body: TypeId,
        origin: Span,
        items: &mut Vec<Symbol>,
    ) {
        let types = &builder.analysis;
        let mut pending = vec![(body, false, origin)];
        let mut visited = HashSet::new();
        while let Some((ty, nested, use_span)) = pending.pop() {
            if !visited.insert((ty, nested, use_span)) {
                continue;
            }

            let shape = types.expect_type_shape(ty);
            if (nested || matches!(shape, TypeShape::Ref(_)))
                && let Some(name) = types.public_name(ty)
            {
                items.push(name);
                if let Ok(owner) = self.source(name)
                    && owner != origin.source
                {
                    self.uses.push((use_span, owner));
                }
                continue;
            }

            match shape {
                TypeShape::Record(fields) => {
                    for (&field, info) in fields {
                        let span = builder.field_origin(ty, field, origin);
                        pending.push((info.final_type, true, span));
                    }
                }
                TypeShape::Variant(cases) => {
                    for payload in cases.values().filter_map(|case| case.type_id()) {
                        pending.push((payload, false, use_span));
                    }
                }
                _ => pending.extend(shape.child_type_ids().map(|child| (child, true, use_span))),
            }
        }
    }
}
