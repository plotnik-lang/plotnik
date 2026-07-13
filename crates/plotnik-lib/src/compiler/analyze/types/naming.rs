//! Type naming pass: every output type gets its name at compile time.
//!
//! Runs after inference, before the analysis freezes. Names come from exactly
//! three places, mirroring where output syntax is written:
//!
//! - A definition's result type carries the definition's name.
//! - A composite reached through field `f` of a type named `T` is `T` +
//!   PascalCase(`f`), landing on array/optional *elements*. Variant-case
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
use crate::compiler::analyze::types::type_shape::{TYPE_VOID, TypeId, TypeShape};
use crate::compiler::diagnostics::report::{DiagnosticKind, Diagnostics};
use crate::compiler::diagnostics::span::Span;
use crate::compiler::parse::ast;
use crate::core::utils::to_pascal_case;
use crate::core::{Interner, Symbol};

struct Claim {
    /// The type this name stands for; `None` reserves the name without a type
    /// (builtins, void definitions).
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

        self.ctx.set_type_names(self.names);
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

    /// Pass 1: every definition claims its name. Non-void results enter the
    /// name table; void definitions still reserve the name (predictability —
    /// a custom capture type may not squat on a definition's name).
    fn claim_definitions(&mut self, symbol_table: &SymbolTable, deps: &DependencyAnalysis) {
        for &def_id in deps.sccs().iter().flatten() {
            let name_sym = deps.def_name_sym(def_id);
            let output = self
                .ctx
                .in_progress()
                .def_output(def_id)
                .expect("every definition is inferred before naming");

            let span = definition_name_span(symbol_table, self.interner, name_sym);
            let type_id = (output != TYPE_VOID).then_some(output);

            if self.claim(name_sym, type_id, span)
                && let Some(type_id) = type_id
            {
                self.names.insert(type_id, name_sym);
            }
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
            if output == TYPE_VOID {
                continue;
            }
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
                    let Some(TypeShape::Record(payload_fields)) =
                        self.ctx.in_progress().type_shape(*payload).cloned()
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

    /// Name the composite (if any) behind a field's type, unwrapping array and
    /// optional wrappers so the name lands on the element.
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
                    // A definition's result keeps its own name here (`(Foo) @val`
                    // renders as `val: Foo`); a custom capture type on it is
                    // redundant noise or a futile rename.
                    self.check_capture_type_on_named(element, existing);
                    return;
                }

                let generated = format!("{parent_name}{}", to_pascal_case(field));
                let (name_sym, span) = match self.custom_capture_types.get(&element).copied() {
                    Some(capture_type) => {
                        if self.interner.resolve(capture_type.name) == generated {
                            self.warn_redundant_capture_type(
                                capture_type.span,
                                "matches the generated type name; omit it",
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
            TypeShape::Custom(sym) => {
                // A leaf alias from `@x :: TypeName`. `:: Node` restates the
                // default; leave it unnamed so it renders as plain `Node`.
                if self.interner.resolve(sym) == "Node" {
                    if let Some(capture_type) = self.custom_capture_types.get(&element).copied() {
                        self.warn_redundant_capture_type(
                            capture_type.span,
                            "`Node` is already the default type; omit it",
                        );
                    }
                    return;
                }
                let span = self
                    .custom_capture_types
                    .get(&element)
                    .map(|capture_type| capture_type.span);
                if self.claim(sym, Some(element), span) {
                    self.names.insert(element, sym);
                }
            }
            TypeShape::Ref(_) => {
                // Named by its own definition; a capture type cannot rename it.
                if let Some(capture_type) = self.custom_capture_types.get(&element).copied() {
                    self.warn_redundant_capture_type(
                        capture_type.span,
                        "a reference keeps its definition's type name; omit it",
                    );
                }
            }
            TypeShape::Void
            | TypeShape::Node
            | TypeShape::Text
            | TypeShape::Bool
            | TypeShape::Array { .. }
            | TypeShape::Option(_) => {}
        }
    }

    fn check_capture_type_on_named(&mut self, element: TypeId, existing: Symbol) {
        let Some(capture_type) = self.custom_capture_types.get(&element).copied() else {
            return;
        };
        if capture_type.name == existing {
            self.warn_redundant_capture_type(
                capture_type.span,
                "restates the type's own name; omit it",
            );
        } else {
            self.warn_redundant_capture_type(
                capture_type.span,
                "cannot rename a type that already has a name; omit it",
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
                Some(TypeShape::Array { element, .. }) => current = *element,
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
