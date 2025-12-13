//! Type inference for Query's BuildGraph.
//!
//! Analyzes the graph and infers output type structure for each definition.
//! Follows rules from ADR-0007 and ADR-0009.
//!
//! # Algorithm Overview
//!
//! 1. Walk graph from each definition entry point
//! 2. Track "pending value" - the captured value waiting for a Field assignment
//! 3. When Field(name) is encountered, record the pending value as a field
//! 4. Handle branching by merging field sets from all branches (1-level merge)
//! 5. Handle quantifiers via array cardinality markers

use std::collections::HashSet;

use indexmap::IndexMap;
use rowan::TextRange;

use crate::diagnostics::{DiagnosticKind, Diagnostics};
use crate::ir::{TYPE_NODE, TYPE_STR, TYPE_VOID, TypeId, TypeKind};

use super::Query;
use super::build_graph::{BuildEffect, BuildGraph, NodeId};

/// Result of type inference.
#[derive(Debug, Default)]
pub struct TypeInferenceResult<'src> {
    pub type_defs: Vec<InferredTypeDef<'src>>,
    pub entrypoint_types: IndexMap<&'src str, TypeId>,
    pub diagnostics: Diagnostics,
    pub errors: Vec<UnificationError<'src>>,
}

/// Error when types cannot be unified in alternation branches.
#[derive(Debug, Clone)]
pub struct UnificationError<'src> {
    pub field: &'src str,
    pub definition: &'src str,
    pub types_found: Vec<TypeDescription>,
    pub spans: Vec<TextRange>,
}

/// Human-readable type description for error messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeDescription {
    Node,
    String,
    Struct(Vec<String>),
}

impl std::fmt::Display for TypeDescription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TypeDescription::Node => write!(f, "Node"),
            TypeDescription::String => write!(f, "String"),
            TypeDescription::Struct(fields) => {
                write!(f, "Struct {{ {} }}", fields.join(", "))
            }
        }
    }
}

/// An inferred type definition.
#[derive(Debug, Clone)]
pub struct InferredTypeDef<'src> {
    pub kind: TypeKind,
    pub name: Option<&'src str>,
    pub members: Vec<InferredMember<'src>>,
    pub inner_type: Option<TypeId>,
}

/// A field (for Record) or variant (for Enum).
#[derive(Debug, Clone)]
pub struct InferredMember<'src> {
    pub name: &'src str,
    pub ty: TypeId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cardinality {
    One,
    Optional,
    Star,
    Plus,
}

impl Cardinality {
    fn join(self, other: Cardinality) -> Cardinality {
        use Cardinality::*;
        match (self, other) {
            (One, One) => One,
            (One, Optional) | (Optional, One) | (Optional, Optional) => Optional,
            (Plus, Plus) => Plus,
            (One, Plus) | (Plus, One) => Plus,
            _ => Star,
        }
    }

    fn make_optional(self) -> Cardinality {
        use Cardinality::*;
        match self {
            One => Optional,
            Plus => Star,
            x => x,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
enum TypeShape<'src> {
    Primitive(TypeId),
    Struct(Vec<&'src str>),
}

impl<'src> TypeShape<'src> {
    fn to_description(&self) -> TypeDescription {
        match self {
            TypeShape::Primitive(TYPE_NODE) => TypeDescription::Node,
            TypeShape::Primitive(TYPE_STR) => TypeDescription::String,
            TypeShape::Primitive(_) => TypeDescription::Node,
            TypeShape::Struct(fields) => {
                TypeDescription::Struct(fields.iter().map(|s| s.to_string()).collect())
            }
        }
    }
}

#[derive(Debug, Clone)]
struct FieldInfo<'src> {
    shape: TypeShape<'src>,
    base_type: TypeId,
    cardinality: Cardinality,
    branch_count: usize,
    all_shapes: Vec<TypeShape<'src>>,
    spans: Vec<TextRange>,
}

#[derive(Debug, Clone, Default)]
struct ScopeInfo<'src> {
    fields: IndexMap<&'src str, FieldInfo<'src>>,
    variants: IndexMap<&'src str, ScopeInfo<'src>>,
    has_variants: bool,
}

impl<'src> ScopeInfo<'src> {
    fn add_field(
        &mut self,
        name: &'src str,
        base_type: TypeId,
        cardinality: Cardinality,
        span: TextRange,
    ) {
        let shape = TypeShape::Primitive(base_type);
        if let Some(existing) = self.fields.get_mut(name) {
            existing.cardinality = existing.cardinality.join(cardinality);
            existing.branch_count += 1;
            if !existing.all_shapes.contains(&shape) {
                existing.all_shapes.push(shape.clone());
            }
            existing.spans.push(span);
        } else {
            self.fields.insert(
                name,
                FieldInfo {
                    shape: shape.clone(),
                    base_type,
                    cardinality,
                    branch_count: 1,
                    all_shapes: vec![shape],
                    spans: vec![span],
                },
            );
        }
    }

    fn merge_from(&mut self, other: ScopeInfo<'src>) -> Vec<MergeError<'src>> {
        let mut errors = Vec::new();

        for (name, info) in other.fields {
            if let Some(existing) = self.fields.get_mut(name) {
                if let Some(mut err) = check_compatibility(&existing.shape, &info.shape, name) {
                    err.spans = existing.spans.clone();
                    err.spans.extend(info.spans.iter().cloned());
                    errors.push(err);
                    for shape in &info.all_shapes {
                        if !existing.all_shapes.contains(shape) {
                            existing.all_shapes.push(shape.clone());
                        }
                    }
                }
                existing.spans.extend(info.spans);
                existing.cardinality = existing.cardinality.join(info.cardinality);
                existing.branch_count += info.branch_count;
            } else {
                self.fields.insert(name, info);
            }
        }

        for (tag, variant_info) in other.variants {
            if let Some(existing) = self.variants.get_mut(tag) {
                let variant_errors = existing.merge_from(variant_info);
                errors.extend(variant_errors);
            } else {
                self.variants.insert(tag, variant_info);
            }
            self.has_variants = true;
        }

        errors
    }

    fn apply_optionality(&mut self, total_branches: usize) {
        for info in self.fields.values_mut() {
            if info.branch_count < total_branches {
                info.cardinality = info.cardinality.make_optional();
            }
        }
    }
}

#[derive(Debug)]
struct MergeError<'src> {
    field: &'src str,
    shapes: Vec<TypeShape<'src>>,
    spans: Vec<TextRange>,
}

fn check_compatibility<'src>(
    a: &TypeShape<'src>,
    b: &TypeShape<'src>,
    field: &'src str,
) -> Option<MergeError<'src>> {
    match (a, b) {
        (TypeShape::Primitive(t1), TypeShape::Primitive(t2)) if t1 == t2 => None,
        (TypeShape::Primitive(_), TypeShape::Primitive(_)) => Some(MergeError {
            field,
            shapes: vec![a.clone(), b.clone()],
            spans: vec![],
        }),
        (TypeShape::Struct(_), TypeShape::Primitive(_))
        | (TypeShape::Primitive(_), TypeShape::Struct(_)) => Some(MergeError {
            field,
            shapes: vec![a.clone(), b.clone()],
            spans: vec![],
        }),
        (TypeShape::Struct(fields_a), TypeShape::Struct(fields_b)) => {
            if fields_a == fields_b {
                None
            } else {
                Some(MergeError {
                    field,
                    shapes: vec![a.clone(), b.clone()],
                    spans: vec![],
                })
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct TraversalState<'src> {
    pending_type: Option<TypeId>,
    cardinality: Cardinality,
    current_variant: Option<&'src str>,
    object_depth: u32,
}

impl Default for TraversalState<'_> {
    fn default() -> Self {
        Self {
            pending_type: None,
            cardinality: Cardinality::One,
            current_variant: None,
            object_depth: 0,
        }
    }
}

struct InferenceContext<'src, 'g> {
    graph: &'g BuildGraph<'src>,
    dead_nodes: &'g HashSet<NodeId>,
    type_defs: Vec<InferredTypeDef<'src>>,
    next_type_id: TypeId,
    diagnostics: Diagnostics,
    errors: Vec<UnificationError<'src>>,
}

impl<'src, 'g> InferenceContext<'src, 'g> {
    fn new(graph: &'g BuildGraph<'src>, dead_nodes: &'g HashSet<NodeId>) -> Self {
        Self {
            graph,
            dead_nodes,
            type_defs: Vec::new(),
            next_type_id: 3, // TYPE_COMPOSITE_START
            diagnostics: Diagnostics::new(),
            errors: Vec::new(),
        }
    }

    fn alloc_type_id(&mut self) -> TypeId {
        let id = self.next_type_id;
        self.next_type_id += 1;
        id
    }

    fn infer_definition(&mut self, def_name: &'src str, entry_id: NodeId) -> TypeId {
        let mut visited = HashSet::new();
        let mut merge_errors = Vec::new();
        let scope = self.traverse(
            entry_id,
            TraversalState::default(),
            &mut visited,
            0,
            &mut merge_errors,
        );

        for err in merge_errors {
            let types_str = err
                .shapes
                .iter()
                .map(|s| s.to_description().to_string())
                .collect::<Vec<_>>()
                .join(" vs ");

            let primary_span = err.spans.first().copied().unwrap_or_default();
            let mut builder = self
                .diagnostics
                .report(DiagnosticKind::IncompatibleTypes, primary_span)
                .message(types_str);

            for span in err.spans.iter().skip(1) {
                builder = builder.related_to("also captured here", *span);
            }
            builder
                .hint(format!(
                    "capture `{}` has incompatible types across branches",
                    err.field
                ))
                .emit();

            self.errors.push(UnificationError {
                field: err.field,
                definition: def_name,
                types_found: err.shapes.iter().map(|s| s.to_description()).collect(),
                spans: err.spans,
            });
        }

        if scope.has_variants && !scope.variants.is_empty() {
            self.create_enum_type(def_name, &scope)
        } else if !scope.fields.is_empty() {
            self.create_struct_type(def_name, &scope)
        } else {
            TYPE_VOID
        }
    }

    fn traverse(
        &mut self,
        node_id: NodeId,
        mut state: TraversalState<'src>,
        visited: &mut HashSet<NodeId>,
        depth: usize,
        errors: &mut Vec<MergeError<'src>>,
    ) -> ScopeInfo<'src> {
        if self.dead_nodes.contains(&node_id) || depth > 200 {
            return ScopeInfo::default();
        }

        if !visited.insert(node_id) && depth > 50 {
            return ScopeInfo::default();
        }

        let node = self.graph.node(node_id);
        let mut scope = ScopeInfo::default();

        for effect in &node.effects {
            match effect {
                BuildEffect::CaptureNode => {
                    state.pending_type = Some(TYPE_NODE);
                }
                BuildEffect::ToString => {
                    state.pending_type = Some(TYPE_STR);
                }
                BuildEffect::Field { name, span } => {
                    if let Some(base_type) = state.pending_type.take() {
                        if let Some(tag) = state.current_variant {
                            let variant_scope = scope.variants.entry(tag).or_default();
                            variant_scope.add_field(*name, base_type, state.cardinality, *span);
                        } else {
                            scope.add_field(*name, base_type, state.cardinality, *span);
                        }
                    }
                    state.cardinality = Cardinality::One;
                }
                BuildEffect::StartArray => {}
                BuildEffect::PushElement => {}
                BuildEffect::EndArray => {
                    state.cardinality = Cardinality::Star;
                }
                BuildEffect::StartObject => {
                    state.object_depth += 1;
                }
                BuildEffect::EndObject => {
                    state.object_depth = state.object_depth.saturating_sub(1);
                }
                BuildEffect::StartVariant(tag) => {
                    state.current_variant = Some(*tag);
                    scope.has_variants = true;
                }
                BuildEffect::EndVariant => {
                    if let Some(tag) = state.current_variant.take() {
                        scope.variants.entry(tag).or_default();
                    }
                }
            }
        }

        let live_successors: Vec<_> = node
            .successors
            .iter()
            .filter(|s| !self.dead_nodes.contains(s))
            .copied()
            .collect();

        if live_successors.is_empty() {
            // Terminal node
        } else if live_successors.len() == 1 {
            let child_scope = self.traverse(live_successors[0], state, visited, depth + 1, errors);
            let merge_errors = scope.merge_from(child_scope);
            errors.extend(merge_errors);
        } else {
            let total_branches = live_successors.len();
            for succ in live_successors {
                let child_scope = self.traverse(succ, state, visited, depth + 1, errors);
                let merge_errors = scope.merge_from(child_scope);
                errors.extend(merge_errors);
            }
            scope.apply_optionality(total_branches);
        }

        scope
    }

    fn create_struct_type(&mut self, name: &'src str, scope: &ScopeInfo<'src>) -> TypeId {
        let members: Vec<_> = scope
            .fields
            .iter()
            .map(|(field_name, info)| {
                let member_type = self.wrap_with_cardinality(info.base_type, info.cardinality);
                InferredMember {
                    name: field_name,
                    ty: member_type,
                }
            })
            .collect();

        let type_id = self.alloc_type_id();

        self.type_defs.push(InferredTypeDef {
            kind: TypeKind::Record,
            name: Some(name),
            members,
            inner_type: None,
        });

        type_id
    }

    fn create_enum_type(&mut self, name: &'src str, scope: &ScopeInfo<'src>) -> TypeId {
        let mut members = Vec::new();
        for (tag, variant_scope) in &scope.variants {
            let variant_type = if variant_scope.fields.is_empty() {
                TYPE_VOID
            } else {
                let variant_name = format!("{}{}", name, tag);
                let leaked: &'src str = Box::leak(variant_name.into_boxed_str());
                self.create_struct_type(leaked, variant_scope)
            };
            members.push(InferredMember {
                name: tag,
                ty: variant_type,
            });
        }

        let type_id = self.alloc_type_id();

        self.type_defs.push(InferredTypeDef {
            kind: TypeKind::Enum,
            name: Some(name),
            members,
            inner_type: None,
        });

        type_id
    }

    fn wrap_with_cardinality(&mut self, base: TypeId, card: Cardinality) -> TypeId {
        match card {
            Cardinality::One => base,
            Cardinality::Optional => {
                let type_id = self.alloc_type_id();
                self.type_defs.push(InferredTypeDef {
                    kind: TypeKind::Optional,
                    name: None,
                    members: Vec::new(),
                    inner_type: Some(base),
                });
                type_id
            }
            Cardinality::Star => {
                let type_id = self.alloc_type_id();
                self.type_defs.push(InferredTypeDef {
                    kind: TypeKind::ArrayStar,
                    name: None,
                    members: Vec::new(),
                    inner_type: Some(base),
                });
                type_id
            }
            Cardinality::Plus => {
                let type_id = self.alloc_type_id();
                self.type_defs.push(InferredTypeDef {
                    kind: TypeKind::ArrayPlus,
                    name: None,
                    members: Vec::new(),
                    inner_type: Some(base),
                });
                type_id
            }
        }
    }
}

impl<'a> Query<'a> {
    /// Run type inference on the graph.
    pub(super) fn infer_types(&mut self) {
        let mut ctx = InferenceContext::new(&self.graph, &self.dead_nodes);

        for (name, entry_id) in self.graph.definitions() {
            let type_id = ctx.infer_definition(name, entry_id);
            self.type_info.entrypoint_types.insert(name, type_id);
        }

        self.type_info.type_defs = ctx.type_defs;
        self.type_info.diagnostics = ctx.diagnostics;
        self.type_info.errors = ctx.errors;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Dump helpers
// ─────────────────────────────────────────────────────────────────────────────

impl TypeInferenceResult<'_> {
    pub fn dump(&self) -> String {
        let mut out = String::new();

        out.push_str("=== Entrypoints ===\n");
        for (name, type_id) in &self.entrypoint_types {
            out.push_str(&format!("{} → {}\n", name, format_type_id(*type_id)));
        }

        if !self.type_defs.is_empty() {
            out.push_str("\n=== Types ===\n");
            for (idx, def) in self.type_defs.iter().enumerate() {
                let type_id = idx as TypeId + 3;
                let name = def.name.unwrap_or("<anon>");
                out.push_str(&format!("T{}: {:?} {}", type_id, def.kind, name));

                if let Some(inner) = def.inner_type {
                    out.push_str(&format!(" → {}", format_type_id(inner)));
                }

                if !def.members.is_empty() {
                    out.push_str(" {\n");
                    for member in &def.members {
                        out.push_str(&format!(
                            "    {}: {}\n",
                            member.name,
                            format_type_id(member.ty)
                        ));
                    }
                    out.push('}');
                }
                out.push('\n');
            }
        }

        if !self.errors.is_empty() {
            out.push_str("\n=== Errors ===\n");
            for err in &self.errors {
                out.push_str(&format!(
                    "field `{}` in `{}`: incompatible types [{}]\n",
                    err.field,
                    err.definition,
                    err.types_found
                        .iter()
                        .map(|t| t.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }

        out
    }

    pub fn dump_diagnostics(&self, source: &str) -> String {
        self.diagnostics.render_filtered(source)
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics.has_errors()
    }
}

fn format_type_id(id: TypeId) -> String {
    if id == TYPE_VOID {
        "Void".to_string()
    } else if id == TYPE_NODE {
        "Node".to_string()
    } else if id == TYPE_STR {
        "String".to_string()
    } else {
        format!("T{}", id)
    }
}
