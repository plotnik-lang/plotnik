//! Type naming pass: every output type gets its name at compile time.
//!
//! Runs after inference, before the analysis freezes. Names come from exactly
//! three places, mirroring where output syntax is written:
//!
//! - A definition's result type carries the definition's name.
//! - A composite reached through field `f` of a type named `T` is `T` +
//!   PascalCase(`f`), landing on array/optional *elements*. Enum variant
//!   payload structs stay anonymous (rendered inline); composites inside a
//!   payload field are named enum name + verbatim label + PascalCase(field).
//! - A `:: TypeName` annotation overrides the generated name and restarts the
//!   chain below it.
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
use crate::compiler::analyze::types::type_analysis::{TypeAnalysisBuilder, TypeAnnotation};
use crate::compiler::analyze::types::type_shape::{TYPE_VOID, TypeId, TypeShape};
use crate::compiler::diagnostics::report::{DiagnosticKind, Diagnostics};
use crate::compiler::diagnostics::span::Span;
use crate::compiler::parse::ast;
use crate::core::utils::to_pascal_case;
use crate::core::{Interner, Symbol};

pub(crate) fn assign_type_names(
    ctx: &mut TypeAnalysisBuilder,
    interner: &mut Interner,
    symbol_table: &SymbolTable,
    dependency_analysis: &DependencyAnalysis,
    diag: &mut Diagnostics,
) {
    let annotated: HashMap<TypeId, TypeAnnotation> = ctx
        .annotations()
        .iter()
        .map(|ann| (ann.type_id, *ann))
        .collect();

    let mut namer = Namer {
        ctx,
        interner,
        diag,
        annotated,
        claims: HashMap::new(),
        names: BTreeMap::new(),
    };

    namer.reserve_builtins();
    namer.claim_definitions(symbol_table, dependency_analysis);
    namer.walk_definitions(dependency_analysis);

    let names = namer.names;
    ctx.set_type_names(names);
}

struct Claim {
    /// The type this name stands for; `None` reserves the name without a type
    /// (builtins, void definitions).
    type_id: Option<TypeId>,
    span: Option<Span>,
}

struct Namer<'a, 'd> {
    ctx: &'a TypeAnalysisBuilder,
    interner: &'a mut Interner,
    diag: &'d mut Diagnostics,
    annotated: HashMap<TypeId, TypeAnnotation>,
    claims: HashMap<Symbol, Claim>,
    names: BTreeMap<TypeId, Symbol>,
}

impl Namer<'_, '_> {
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
    /// an annotation may not squat on a definition's name).
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

    /// Descend a struct/enum that already carries `name`, naming its nested
    /// composites.
    fn walk_named(&mut self, type_id: TypeId, name: &str) {
        let shape = self
            .ctx
            .in_progress()
            .type_shape(type_id)
            .cloned()
            .expect("named type is registered");

        match shape {
            TypeShape::Struct(fields) => {
                for (field_sym, info) in &fields {
                    let field = self.interner.resolve(*field_sym).to_owned();
                    self.visit_field_element(info.type_id, name, &field);
                }
            }
            TypeShape::Enum(variants) => {
                // Variant payload structs are anonymous (rendered inline as the
                // variant's data); composites inside their fields are named
                // through the enum name + the verbatim label.
                for (label_sym, payload) in &variants {
                    let Some(TypeShape::Struct(payload_fields)) =
                        self.ctx.in_progress().type_shape(*payload).cloned()
                    else {
                        continue;
                    };
                    let label = self.interner.resolve(*label_sym).to_owned();
                    let prefix = format!("{name}{label}");
                    for (field_sym, info) in &payload_fields {
                        let field = self.interner.resolve(*field_sym).to_owned();
                        self.visit_field_element(info.type_id, &prefix, &field);
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
            TypeShape::Struct(_) | TypeShape::Enum(_) => {
                if let Some(&existing) = self.names.get(&element) {
                    // A definition's result keeps its own name here (`(Foo) @val`
                    // renders as `val: Foo`); an annotation on it is redundant
                    // noise or a futile rename.
                    self.check_annotation_on_named(element, existing);
                    return;
                }

                let generated = format!("{parent_name}{}", to_pascal_case(field));
                let (name_sym, span) = match self.annotated.get(&element).copied() {
                    Some(ann) => {
                        if self.interner.resolve(ann.name) == generated {
                            self.warn_redundant_annotation(
                                ann.span,
                                "matches the generated type name; omit it",
                            );
                        }
                        (ann.name, Some(ann.span))
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
                    if let Some(ann) = self.annotated.get(&element).copied() {
                        self.warn_redundant_annotation(
                            ann.span,
                            "`Node` is already the default type; omit it",
                        );
                    }
                    return;
                }
                let span = self.annotated.get(&element).map(|ann| ann.span);
                if self.claim(sym, Some(element), span) {
                    self.names.insert(element, sym);
                }
            }
            TypeShape::Ref(_) => {
                // Named by its own definition; an annotation cannot rename it.
                if let Some(ann) = self.annotated.get(&element).copied() {
                    self.warn_redundant_annotation(
                        ann.span,
                        "a reference keeps its definition's type name; omit it",
                    );
                }
            }
            TypeShape::Void
            | TypeShape::Node
            | TypeShape::Array { .. }
            | TypeShape::Optional(_) => {}
        }
    }

    fn check_annotation_on_named(&mut self, element: TypeId, existing: Symbol) {
        let Some(ann) = self.annotated.get(&element).copied() else {
            return;
        };
        if ann.name == existing {
            self.warn_redundant_annotation(ann.span, "restates the type's own name; omit it");
        } else {
            self.warn_redundant_annotation(
                ann.span,
                "cannot rename a type that already has a name; omit it",
            );
        }
    }

    fn warn_redundant_annotation(&mut self, span: Span, detail: &str) {
        self.diag
            .report(DiagnosticKind::RedundantTypeAnnotation, span)
            .detail(detail)
            .emit();
    }

    fn unwrap_wrappers(&self, type_id: TypeId) -> TypeId {
        let mut current = type_id;
        loop {
            match self.ctx.in_progress().type_shape(current) {
                Some(TypeShape::Array { element, .. }) => current = *element,
                Some(TypeShape::Optional(inner)) => current = *inner,
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
                let Some(span) = span.or(existing_span) else {
                    // No span on either side cannot happen for user-written
                    // conflicts: builtins never conflict with each other.
                    unreachable!("a name conflict always involves a user-written site");
                };
                let mut builder = self
                    .diag
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
