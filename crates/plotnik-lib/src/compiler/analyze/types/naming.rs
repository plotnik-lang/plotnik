//! Type naming pass: every result type gets its name at compile time.
//!
//! Runs after inference, before the analysis freezes. Names come from exactly
//! three places, mirroring where result-producing syntax is written:
//!
//! - A definition's result type carries the definition's name.
//! - A composite reached through field `f` of a type named `T` is `T` +
//!   PascalCase(`f`), landing on list/option *elements*. Variant-case
//!   payload records stay anonymous (rendered inline); composites inside a
//!   payload fields are named variant type name + verbatim label + PascalCase(field).
//! - A custom `:: TypeName` capture type overrides the generated name and
//!   restarts the chain below it.
//!
//! Names are nominal: the same name may recur only for structurally identical
//! types (they collapse into one emission). Any other collision is a compile
//! error carrying both sites. `Node` and definition names are reserved.
//! Downstream (bytecode emission, typegen) renders names; it never invents
//! them.

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap};

use crate::compiler::analyze::names::SymbolTable;
use crate::compiler::analyze::refs::DependencyAnalysis;
use crate::compiler::analyze::types::type_analysis::{
    CustomCaptureTypeOccurrence, TypeAnalysisBuilder,
};
use crate::compiler::analyze::types::type_shape::{TypeId, TypeShape};
use crate::compiler::diagnostics::report::{DiagnosticKind, Diagnostics};
use crate::compiler::diagnostics::span::Span;
use crate::compiler::parse::ast;
use crate::core::utils::to_pascal_case;
use crate::core::{Interner, Symbol};

struct Claim {
    /// The type this name stands for; `None` reserves the name without a type
    /// (builtins, match-only definitions).
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
        }
    }

    pub(crate) fn assign(
        mut self,
        symbol_table: &SymbolTable,
        dependency_analysis: &DependencyAnalysis,
    ) {
        self.reserve_builtins();
        self.claim_definitions(symbol_table, dependency_analysis);
        self.walk_definitions(dependency_analysis);

        self.ctx.set_named_types(self.names);
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

    /// Pass 1: every definition claims its declaration name. Definition names
    /// stay attached to their declarations rather than to an interned body
    /// shape; match-only definitions still reserve the name.
    fn claim_definitions(&mut self, symbol_table: &SymbolTable, deps: &DependencyAnalysis) {
        for &def_id in deps.sccs().iter().flatten() {
            let name_sym = deps.def_name_sym(def_id);
            let output = self
                .ctx
                .in_progress()
                .def_output(def_id)
                .expect("every definition is inferred before naming");

            let span = definition_name_span(symbol_table, self.interner, name_sym);
            let type_id = output.value();

            self.claim(name_sym, type_id, span);
        }
    }

    /// Pass 2: descend each definition's result, naming nested composites.
    fn walk_definitions(&mut self, deps: &DependencyAnalysis) {
        for &def_id in deps.sccs().iter().flatten() {
            let output = self
                .ctx
                .in_progress()
                .def_output(def_id)
                .expect("every definition is inferred before naming");
            let Some(output) = output.value() else {
                continue;
            };
            let name = self.interner.resolve(deps.def_name_sym(def_id)).to_owned();
            self.walk_named(output, &name);
        }
    }

    /// Descend a record/variant type that already carries `name`, naming its nested
    /// composites.
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
                // Variant payload records are anonymous (rendered inline as the
                // case's data); composites inside their fields are named
                // through the variant type name + the verbatim label.
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

    /// Name the composite (if any) behind a field's type, unwrapping list and
    /// option wrappers so the name lands on the element.
    fn visit_field_element(&mut self, field_type: TypeId, parent_name: &str, field: &str) {
        let element = self.unwrap_wrappers(field_type);
        let shape = self
            .ctx
            .in_progress()
            .type_shape(element)
            .cloned()
            .expect("field element type is registered");

        match shape {
            TypeShape::Record(_) | TypeShape::Variant(_) => {
                if let Some(&existing) = self.names.get(&element) {
                    // The composite already has a generated or explicit name;
                    // a second capture type is redundant noise or a futile
                    // rename.
                    self.check_capture_type_on_named(element, existing);
                    return;
                }

                let generated = format!("{parent_name}{}", to_pascal_case(field));
                let (name_sym, span) = match self.custom_capture_types.get(&element).copied() {
                    Some(capture_type) => {
                        if self.interner.resolve(capture_type.name) == generated {
                            self.warn_redundant_capture_type(
                                capture_type.span,
                                &format!(
                                    "this capture already has type `{generated}`; naming it `{generated}` won't have an effect"
                                ),
                            );
                        }
                        (capture_type.name, Some(capture_type.span))
                    }
                    None => {
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
                    // A definition reference already has its declaration's name;
                    // a capture type cannot rename it.
                    let Some(capture_type) = self.custom_capture_types.get(&element).copied()
                    else {
                        return;
                    };
                    let declaration_name = self.interner.resolve(declaration_name).to_owned();
                    let written_name = self.interner.resolve(capture_type.name).to_owned();
                    let detail = format!(
                        "this capture already has type `{declaration_name}`; naming it `{written_name}` won't have an effect"
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
                    "this capture already has type `Node`; naming it `Node` won't have an effect",
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
                    "this capture already has type `{existing}`; naming it `{existing}` won't have an effect"
                ),
            );
        } else {
            let existing = self.interner.resolve(existing);
            let written = self.interner.resolve(capture_type.name);
            self.warn_redundant_capture_type(
                capture_type.span,
                &format!(
                    "this capture already has type `{existing}`; naming it `{written}` won't have an effect"
                ),
            );
        }
    }

    fn warn_redundant_capture_type(&mut self, span: Span, detail: &str) {
        let Some(diag) = self.diag.as_deref_mut() else {
            return;
        };
        diag.report(DiagnosticKind::RedundantCaptureType, span)
            .detail(detail)
            .emit();
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

    /// Claim `name` for `type_id`. The same name may recur for structurally
    /// identical types (nominal identity: one name, one shape); anything else
    /// is a compile error at the claimant's site, pointing back at the first.
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
                    // No span on either side cannot happen for user-written
                    // conflicts: builtins never conflict with each other.
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

/// Raw naming validation records exactly which types participate in a naming
/// conflict so capture-type normalization cannot erase that error. It does not
/// emit diagnostics or install names; the final naming pass reports raw and
/// normalized-only conflicts once against the final graph.
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
        symbol_table: &SymbolTable,
        dependency_analysis: &DependencyAnalysis,
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
        };
        validator.reserve_builtins();
        validator.claim_definitions(symbol_table, dependency_analysis);
        validator.walk_definitions(dependency_analysis);
    }
}

/// The span of a definition's name token (falling back to its body).
fn definition_name_span(
    symbol_table: &SymbolTable,
    interner: &Interner,
    name_sym: Symbol,
) -> Option<Span> {
    let name = interner.resolve(name_sym);
    let (source, body) = symbol_table.definition(name)?;
    let range = body
        .syntax()
        .parent()
        .and_then(ast::Def::cast)
        .and_then(|def| def.name())
        .map(|tok| tok.text_range())
        .unwrap_or_else(|| body.text_range());
    Some(Span::new(source, range))
}
