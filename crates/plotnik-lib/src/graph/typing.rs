//! Type inference for BuildGraph.
//!
//! This module analyzes a BuildGraph and infers the output type structure
//! for each definition. The inference follows rules from ADR-0007 and ADR-0009.
//!
//! # Algorithm Overview
//!
//! 1. Walk graph from each definition entry point
//! 2. Track "pending value" - the captured value waiting for a Field assignment
//! 3. When Field(name) is encountered, record the pending value as a field
//! 4. Handle branching by merging field sets from all branches (1-level merge)
//! 5. Handle quantifiers via array cardinality markers
//!
//! # 1-Level Merge Semantics
//!
//! When merging captures across alternation branches:
//! - Top-level fields merge with optionality for asymmetric captures
//! - Base types (Node, String) must match exactly
//! - Nested structs must be structurally identical (not recursively merged)
//! - All incompatibilities are reported, not just the first

use super::{BuildEffect, BuildGraph, NodeId};
use crate::diagnostics::{DiagnosticKind, Diagnostics};
use crate::ir::{TYPE_NODE, TYPE_STR, TYPE_VOID, TypeId, TypeKind};
use indexmap::IndexMap;
use rowan::TextRange;
use std::collections::HashSet;

/// Result of type inference on a BuildGraph.
#[derive(Debug)]
pub struct TypeInferenceResult<'src> {
    /// All inferred type definitions (composite types only).
    pub type_defs: Vec<InferredTypeDef<'src>>,
    /// Mapping from definition name to its result TypeId.
    pub entrypoint_types: IndexMap<&'src str, TypeId>,
    /// Type inference diagnostics.
    pub diagnostics: Diagnostics,
    /// Type unification errors (incompatible types in alternation branches).
    /// Kept for backward compatibility; diagnostics is the primary error channel.
    pub errors: Vec<UnificationError<'src>>,
}

/// Error when types cannot be unified in alternation branches.
#[derive(Debug, Clone)]
pub struct UnificationError<'src> {
    /// The field name where incompatibility was detected.
    pub field: &'src str,
    /// Definition context where the error occurred.
    pub definition: &'src str,
    /// Types found across branches (for error message).
    pub types_found: Vec<TypeDescription>,
    /// Spans of the conflicting captures.
    pub spans: Vec<TextRange>,
}

/// Human-readable type description for error messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeDescription {
    Node,
    String,
    Struct(Vec<String>), // field names for identification
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

/// An inferred type definition (before emission).
#[derive(Debug, Clone)]
pub struct InferredTypeDef<'src> {
    pub kind: TypeKind,
    pub name: Option<&'src str>,
    /// For Record/Enum: fields or variants. For wrappers: empty.
    pub members: Vec<InferredMember<'src>>,
    /// For wrapper types: the inner TypeId.
    pub inner_type: Option<TypeId>,
}

/// A field (for Record) or variant (for Enum).
#[derive(Debug, Clone)]
pub struct InferredMember<'src> {
    pub name: &'src str,
    pub ty: TypeId,
}

/// Cardinality of a capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cardinality {
    One,
    Optional,
    Star,
    Plus,
}

impl Cardinality {
    /// Join cardinalities (for alternation branches).
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

    /// Make optional (for fields missing in some alternation branches).
    fn make_optional(self) -> Cardinality {
        use Cardinality::*;
        match self {
            One => Optional,
            Plus => Star,
            x => x,
        }
    }
}

/// Type shape for 1-level merge comparison.
/// Tracks enough information to detect incompatibilities.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // Struct variant is infrastructure for captured sequence support
enum TypeShape<'src> {
    /// Primitive: Node or String
    Primitive(TypeId),
    /// Struct with known field names (for structural identity check)
    Struct(Vec<&'src str>),
}

impl<'src> TypeShape<'src> {
    fn to_description(&self) -> TypeDescription {
        match self {
            TypeShape::Primitive(TYPE_NODE) => TypeDescription::Node,
            TypeShape::Primitive(TYPE_STR) => TypeDescription::String,
            TypeShape::Primitive(_) => TypeDescription::Node, // fallback
            TypeShape::Struct(fields) => {
                TypeDescription::Struct(fields.iter().map(|s| s.to_string()).collect())
            }
        }
    }
}

/// Inferred field information collected during traversal.
#[derive(Debug, Clone)]
struct FieldInfo<'src> {
    /// The inferred type shape (for compatibility checking).
    shape: TypeShape<'src>,
    /// Base TypeId (TYPE_NODE or TYPE_STR for primitives, placeholder for structs).
    base_type: TypeId,
    /// Cardinality from quantifiers.
    cardinality: Cardinality,
    /// Number of branches this field appears in (for optional detection).
    branch_count: usize,
    /// All shapes seen at this field (for error reporting).
    all_shapes: Vec<TypeShape<'src>>,
    /// Spans where this field was captured (for error reporting).
    spans: Vec<TextRange>,
}

/// Collected scope information from traversal.
#[derive(Debug, Clone, Default)]
struct ScopeInfo<'src> {
    fields: IndexMap<&'src str, FieldInfo<'src>>,
    /// Variants for tagged alternations.
    variants: IndexMap<&'src str, ScopeInfo<'src>>,
    /// Whether we've seen variant markers (StartVariant/EndVariant).
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

    /// Merge another scope into this one, applying 1-level merge semantics.
    /// Returns errors for incompatible types.
    /// Note: Does NOT apply optionality - call `apply_optionality` after all branches merged.
    fn merge_from(&mut self, other: ScopeInfo<'src>) -> Vec<MergeError<'src>> {
        let mut errors = Vec::new();

        for (name, info) in other.fields {
            if let Some(existing) = self.fields.get_mut(name) {
                // Check type compatibility (1-level merge)
                if let Some(mut err) = check_compatibility(&existing.shape, &info.shape, name) {
                    // Attach spans from both sides
                    err.spans = existing.spans.clone();
                    err.spans.extend(info.spans.iter().cloned());
                    errors.push(err);
                    // Collect all shapes for error reporting
                    for shape in &info.all_shapes {
                        if !existing.all_shapes.contains(shape) {
                            existing.all_shapes.push(shape.clone());
                        }
                    }
                }
                // Always merge spans
                existing.spans.extend(info.spans);
                existing.cardinality = existing.cardinality.join(info.cardinality);
                existing.branch_count += info.branch_count;
            } else {
                self.fields.insert(name, info);
            }
        }

        // Merge variants
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

    /// Apply optionality to fields that don't appear in all branches.
    /// Must be called after all branches have been merged.
    fn apply_optionality(&mut self, total_branches: usize) {
        for info in self.fields.values_mut() {
            if info.branch_count < total_branches {
                info.cardinality = info.cardinality.make_optional();
            }
        }
    }
}

/// Internal error during merge (before conversion to UnificationError).
#[derive(Debug)]
struct MergeError<'src> {
    field: &'src str,
    shapes: Vec<TypeShape<'src>>,
    spans: Vec<TextRange>,
}

/// Check if two type shapes are compatible under 1-level merge semantics.
fn check_compatibility<'src>(
    a: &TypeShape<'src>,
    b: &TypeShape<'src>,
    field: &'src str,
) -> Option<MergeError<'src>> {
    match (a, b) {
        // Same primitive types are compatible
        (TypeShape::Primitive(t1), TypeShape::Primitive(t2)) if t1 == t2 => None,

        // Different primitives (Node vs String) are incompatible
        (TypeShape::Primitive(_), TypeShape::Primitive(_)) => Some(MergeError {
            field,
            shapes: vec![a.clone(), b.clone()],
            spans: vec![], // Filled in by caller
        }),

        // Struct vs Primitive is incompatible
        (TypeShape::Struct(_), TypeShape::Primitive(_))
        | (TypeShape::Primitive(_), TypeShape::Struct(_)) => Some(MergeError {
            field,
            shapes: vec![a.clone(), b.clone()],
            spans: vec![], // Filled in by caller
        }),

        // Structs: must have identical field sets (1-level, no deep merge)
        (TypeShape::Struct(fields_a), TypeShape::Struct(fields_b)) => {
            if fields_a == fields_b {
                None
            } else {
                Some(MergeError {
                    field,
                    shapes: vec![a.clone(), b.clone()],
                    spans: vec![], // Filled in by caller
                })
            }
        }
    }
}

/// State during graph traversal.
#[derive(Debug, Clone, Copy)]
struct TraversalState<'src> {
    /// The type of the current pending value (after CaptureNode).
    pending_type: Option<TypeId>,
    /// Current cardinality wrapper (from array effects).
    cardinality: Cardinality,
    /// Current variant tag (inside StartVariant..EndVariant).
    current_variant: Option<&'src str>,
    /// Depth counter for nested objects.
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

/// Context for type inference.
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

        // Convert merge errors to unification errors and diagnostics
        for err in merge_errors {
            let types_str = err
                .shapes
                .iter()
                .map(|s| s.to_description().to_string())
                .collect::<Vec<_>>()
                .join(" vs ");

            // Use first span as primary, others as related
            let primary_span = err.spans.first().copied().unwrap_or_default();
            let mut builder = self
                .diagnostics
                .report(DiagnosticKind::IncompatibleTypes, primary_span)
                .message(types_str);

            // Add related spans
            for span in err.spans.iter().skip(1) {
                builder = builder.related_to("also captured here", *span);
            }
            builder
                .hint(format!(
                    "capture `{}` has incompatible types across branches",
                    err.field
                ))
                .emit();

            // Keep legacy error for backward compat
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

        // Cycle detection - but allow revisiting at different depths for quantifiers
        if !visited.insert(node_id) && depth > 50 {
            return ScopeInfo::default();
        }

        let node = self.graph.node(node_id);
        let mut scope = ScopeInfo::default();

        // Process effects on this node
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
                            // Inside a variant - add to variant scope
                            let variant_scope = scope.variants.entry(tag).or_default();
                            variant_scope.add_field(*name, base_type, state.cardinality, *span);
                        } else {
                            scope.add_field(*name, base_type, state.cardinality, *span);
                        }
                    }
                    state.cardinality = Cardinality::One;
                }
                BuildEffect::StartArray => {
                    // Mark that we're collecting into an array
                }
                BuildEffect::PushElement => {
                    // Element pushed to array
                }
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
                        // Ensure variant exists even if empty
                        scope.variants.entry(tag).or_default();
                    }
                }
            }
        }

        // Process successors
        let live_successors: Vec<_> = node
            .successors
            .iter()
            .filter(|s| !self.dead_nodes.contains(s))
            .copied()
            .collect();

        if live_successors.is_empty() {
            // Terminal node
        } else if live_successors.len() == 1 {
            // Linear path - continue with same state
            let child_scope = self.traverse(live_successors[0], state, visited, depth + 1, errors);
            let merge_errors = scope.merge_from(child_scope);
            errors.extend(merge_errors);
        } else {
            // Branching - traverse each branch and merge results
            let total_branches = live_successors.len();
            for succ in live_successors {
                let child_scope = self.traverse(succ, state.clone(), visited, depth + 1, errors);
                let merge_errors = scope.merge_from(child_scope);
                errors.extend(merge_errors);
            }
            // Apply optionality after all branches merged
            scope.apply_optionality(total_branches);
        }

        scope
    }

    fn create_struct_type(&mut self, name: &'src str, scope: &ScopeInfo<'src>) -> TypeId {
        // Create members first - this may allocate wrapper types
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

        // Now allocate struct type_id - this ensures proper ordering
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
        // Create variant payloads first - this may allocate nested types
        let mut members = Vec::new();
        for (tag, variant_scope) in &scope.variants {
            let variant_type = if variant_scope.fields.is_empty() {
                TYPE_VOID
            } else {
                // Create synthetic name for variant payload
                let variant_name = format!("{}{}", name, tag);
                let leaked: &'src str = Box::leak(variant_name.into_boxed_str());
                self.create_struct_type(leaked, variant_scope)
            };
            members.push(InferredMember {
                name: tag,
                ty: variant_type,
            });
        }

        // Now allocate enum type_id - this ensures proper ordering
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

/// Infer types for all definitions in a BuildGraph.
pub fn infer_types<'src>(
    graph: &BuildGraph<'src>,
    dead_nodes: &HashSet<NodeId>,
) -> TypeInferenceResult<'src> {
    let mut ctx = InferenceContext::new(graph, dead_nodes);
    let mut entrypoint_types = IndexMap::new();

    for (name, entry_id) in graph.definitions() {
        let type_id = ctx.infer_definition(name, entry_id);
        entrypoint_types.insert(name, type_id);
    }

    TypeInferenceResult {
        type_defs: ctx.type_defs,
        entrypoint_types,
        diagnostics: ctx.diagnostics,
        errors: ctx.errors,
    }
}
