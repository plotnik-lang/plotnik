use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::compiler::analyze::refs::DefinitionGraph;
use crate::compiler::analyze::types::type_analysis::{
    CustomCaptureTypeOccurrence, TypeAnalysisBuilder,
};
use crate::compiler::analyze::types::type_shape::{TypeId, TypeShape};
use crate::compiler::diagnostics::report::{DiagnosticKind, Diagnostics};
use crate::compiler::diagnostics::span::Span;
use crate::core::utils::to_pascal_case;
use crate::core::{Interner, Symbol};

struct Claim {
    type_id: Option<TypeId>,
    span: Option<Span>,
}

pub(crate) struct TypeNamer<'a, 'd> {
    ctx: &'a mut TypeAnalysisBuilder,
    interner: &'a mut Interner,
    diag: Option<&'d mut Diagnostics>,
    custom_capture_types: HashMap<TypeId, CustomCaptureTypeOccurrence>,
    claims: HashMap<Symbol, Claim>,
    names: BTreeMap<TypeId, Symbol>,
    redundant_capture_types: Vec<(Span, String)>,
}

impl<'a, 'd> TypeNamer<'a, 'd> {
    pub(crate) fn new(
        ctx: &'a mut TypeAnalysisBuilder,
        interner: &'a mut Interner,
        diag: &'d mut Diagnostics,
    ) -> Self {
        let custom_capture_types = ctx
            .custom_capture_types()
            .iter()
            .map(|capture_type| (capture_type.type_id, *capture_type))
            .collect();
        Self {
            ctx,
            interner,
            diag: Some(diag),
            custom_capture_types,
            claims: HashMap::new(),
            names: BTreeMap::new(),
            redundant_capture_types: Vec::new(),
        }
    }

    pub(crate) fn assign(
        mut self,
        definitions: &DefinitionGraph,
        expression: Option<crate::compiler::diagnostics::SourceId>,
    ) {
        self.reserve_builtins();
        self.claim_definitions(definitions);
        self.walk_definitions(definitions);
        self.claim_expression_types(expression);

        self.ctx.set_named_types(self.names);
        let diag = self.diag.expect("final naming has diagnostics");
        let ownership_annotations = if diag.has_errors() {
            HashSet::new()
        } else {
            self.ctx.resolve_public_types(definitions)
        };
        for (span, detail) in self.redundant_capture_types {
            if ownership_annotations.contains(&span) {
                continue;
            }

            diag.report(DiagnosticKind::RedundantCaptureType, span)
                .detail(detail)
                .emit();
        }
    }

    fn claim_expression_types(&mut self, source: Option<crate::compiler::diagnostics::SourceId>) {
        let Some(source) = source else {
            return;
        };

        let mut types: Vec<_> = self
            .custom_capture_types
            .iter()
            .filter(|(_, occurrence)| occurrence.span.source == source)
            .map(|(&ty, _)| ty)
            .collect();
        types.sort();
        for ty in types {
            self.name_element(self.unwrap_wrappers(ty), None);
        }
    }

    fn reserve_builtins(&mut self) {
        let node_sym = self.interner.intern("Node");
        self.claims.insert(
            node_sym,
            Claim {
                type_id: None,
                span: None,
            },
        );
    }

    fn claim_definitions(&mut self, definitions: &DefinitionGraph) {
        for def_id in definitions.ids_in_def_id_order() {
            let definition = definitions.definition(def_id);
            let name_sym = definition.name();
            let output = self
                .ctx
                .in_progress()
                .def_output(def_id)
                .expect("every definition is inferred before naming");

            let span = Some(definition.span());
            let type_id = output.value();

            self.claim(name_sym, type_id, span);
        }
    }

    fn walk_definitions(&mut self, definitions: &DefinitionGraph) {
        for def_id in definitions.ids_in_def_id_order() {
            let output = self
                .ctx
                .in_progress()
                .def_output(def_id)
                .expect("every definition is inferred before naming");
            let Some(output) = output.value() else {
                continue;
            };
            let name = self
                .interner
                .resolve(definitions.definition(def_id).name())
                .to_owned();
            self.walk_named(output, &name);
        }
    }

    fn walk_named(&mut self, type_id: TypeId, name: &str) {
        let shape = self
            .ctx
            .in_progress()
            .type_shape(type_id)
            .cloned()
            .expect("named type is registered");

        match shape {
            TypeShape::Record(fields) => {
                for (field_sym, info) in &fields {
                    let field = self.interner.resolve(*field_sym).to_owned();
                    self.visit_field_element(info.final_type, name, &field);
                }
            }
            TypeShape::Variant(cases) => {
                for (label_sym, payload) in &cases {
                    let Some(payload) = payload.type_id() else {
                        continue;
                    };
                    let Some(TypeShape::Record(payload_fields)) =
                        self.ctx.in_progress().type_shape(payload).cloned()
                    else {
                        continue;
                    };
                    let label = self.interner.resolve(*label_sym).to_owned();
                    let prefix = format!("{name}{label}");
                    for (field_sym, info) in &payload_fields {
                        let field = self.interner.resolve(*field_sym).to_owned();
                        self.visit_field_element(info.final_type, &prefix, &field);
                    }
                }
            }
            _ => {}
        }
    }

    fn visit_field_element(&mut self, field_type: TypeId, parent_name: &str, field: &str) {
        let element = self.unwrap_wrappers(field_type);
        let generated = format!("{parent_name}{}", to_pascal_case(field));
        self.name_element(element, Some(generated));
    }

    fn name_element(&mut self, element: TypeId, generated: Option<String>) {
        let shape = self
            .ctx
            .in_progress()
            .type_shape(element)
            .cloned()
            .expect("field element type is registered");

        match shape {
            TypeShape::Record(_) | TypeShape::Variant(_) => {
                if let Some(&existing) = self.names.get(&element) {
                    self.check_capture_type_on_named(element, existing);
                    return;
                }

                let (name_sym, span) = match self.custom_capture_types.get(&element).copied() {
                    Some(capture_type) => {
                        if generated.as_deref() == Some(self.interner.resolve(capture_type.name)) {
                            self.warn_redundant_capture_type(
                                capture_type.span,
                                &format!(
                                    "this capture already has type `{name}`. Naming it `{name}` has no effect", name = self.interner.resolve(capture_type.name)
                                ),
                            );
                        }
                        (capture_type.name, Some(capture_type.span))
                    }
                    None => {
                        let Some(generated) = generated else {
                            return;
                        };

                        let sym = self.interner.intern(&generated);
                        (sym, self.ctx.type_provenance(element))
                    }
                };

                if self.claim(name_sym, Some(element), span) {
                    self.names.insert(element, name_sym);
                    let name = self.interner.resolve(name_sym).to_owned();
                    self.walk_named(element, &name);
                }
            }
            TypeShape::Ref(declaration) => {
                let declaration_name = self.ctx.in_progress().declaration_name(declaration);
                if self
                    .ctx
                    .in_progress()
                    .declaration_definition(declaration)
                    .is_some()
                {
                    let Some(capture_type) = self.custom_capture_types.get(&element).copied()
                    else {
                        return;
                    };
                    let declaration_name = self.interner.resolve(declaration_name).to_owned();
                    let written_name = self.interner.resolve(capture_type.name).to_owned();
                    let detail = format!(
                        "this capture already has type `{declaration_name}`. Naming it `{written_name}` has no effect"
                    );
                    self.warn_redundant_capture_type(capture_type.span, &detail);
                    return;
                }

                let body = self
                    .ctx
                    .in_progress()
                    .declaration_body(declaration)
                    .expect("capture type declaration has a body");
                let span = self
                    .custom_capture_types
                    .get(&element)
                    .map(|capture_type| capture_type.span);
                if self.claim(declaration_name, Some(body), span) {
                    self.names.insert(element, declaration_name);
                }
            }
            TypeShape::Node => {
                let Some(capture_type) = self.custom_capture_types.get(&element).copied() else {
                    return;
                };
                self.warn_redundant_capture_type(
                    capture_type.span,
                    "this capture already has type `Node`. Naming it `Node` has no effect",
                );
            }
            TypeShape::Text | TypeShape::Bool | TypeShape::List { .. } | TypeShape::Option(_) => {}
        }
    }

    fn check_capture_type_on_named(&mut self, element: TypeId, existing: Symbol) {
        let Some(capture_type) = self.custom_capture_types.get(&element).copied() else {
            return;
        };
        if capture_type.name == existing {
            let existing = self.interner.resolve(existing);
            self.warn_redundant_capture_type(
                capture_type.span,
                &format!(
                    "this capture already has type `{existing}`. Naming it `{existing}` has no effect"
                ),
            );
        } else {
            let existing = self.interner.resolve(existing);
            let written = self.interner.resolve(capture_type.name);
            self.warn_redundant_capture_type(
                capture_type.span,
                &format!(
                    "this capture already has type `{existing}`. Naming it `{written}` has no effect"
                ),
            );
        }
    }

    fn warn_redundant_capture_type(&mut self, span: Span, detail: &str) {
        if self.diag.is_none() {
            return;
        }

        self.redundant_capture_types.push((span, detail.to_owned()));
    }

    fn unwrap_wrappers(&self, type_id: TypeId) -> TypeId {
        let mut current = type_id;
        loop {
            match self.ctx.in_progress().type_shape(current) {
                Some(TypeShape::List { element, .. }) => current = *element,
                Some(TypeShape::Option(inner)) => current = *inner,
                _ => return current,
            }
        }
    }

    fn claim(&mut self, name: Symbol, type_id: Option<TypeId>, span: Option<Span>) -> bool {
        match self.claims.entry(name) {
            Entry::Vacant(slot) => {
                slot.insert(Claim { type_id, span });
                true
            }
            Entry::Occupied(existing) => {
                let existing_claim = existing.get();
                if existing_claim.type_id == type_id {
                    return true;
                }
                if let (Some(a), Some(b)) = (existing_claim.type_id, type_id)
                    && self.ctx.types_structurally_equal(a, b)
                {
                    return true;
                }

                let existing_span = existing_claim.span;
                let name_str = self.interner.resolve(name).to_owned();
                if let Some(type_id) = type_id {
                    self.ctx.record_invalid_type(type_id);
                }
                let Some(span) = span.or(existing_span) else {
                    unreachable!("a name conflict always involves a user-written site");
                };
                let Some(diag) = self.diag.as_deref_mut() else {
                    return false;
                };
                let mut builder = diag
                    .report(DiagnosticKind::TypeNameConflict, span)
                    .detail(name_str);
                if let Some(first) = existing_span
                    && first != span
                {
                    builder = builder.related_to(first, "already used here");
                } else if existing_span.is_none() {
                    builder = builder.hint("`Node` is the builtin node type name");
                }
                builder.emit();
                false
            }
        }
    }
}

pub(crate) struct RawTypeNameValidator<'a> {
    ctx: &'a mut TypeAnalysisBuilder,
    interner: &'a mut Interner,
}

impl<'a> RawTypeNameValidator<'a> {
    pub(crate) fn new(ctx: &'a mut TypeAnalysisBuilder, interner: &'a mut Interner) -> Self {
        Self { ctx, interner }
    }

    pub(crate) fn validate(
        self,
        definitions: &DefinitionGraph,
        expression: Option<crate::compiler::diagnostics::SourceId>,
    ) {
        let custom_capture_types = self
            .ctx
            .custom_capture_types()
            .iter()
            .map(|capture_type| (capture_type.type_id, *capture_type))
            .collect();
        let mut validator = TypeNamer {
            ctx: self.ctx,
            interner: self.interner,
            diag: None,
            custom_capture_types,
            claims: HashMap::new(),
            names: BTreeMap::new(),
            redundant_capture_types: Vec::new(),
        };
        validator.reserve_builtins();
        validator.claim_definitions(definitions);
        validator.walk_definitions(definitions);
        validator.claim_expression_types(expression);
    }
}
