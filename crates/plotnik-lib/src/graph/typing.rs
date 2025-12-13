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
//! 4. Handle branching by merging field sets from all branches
//! 5. Handle quantifiers via array cardinality markers

use super::{BuildEffect, BuildGraph, NodeId};
use crate::ir::{TYPE_NODE, TYPE_STR, TYPE_VOID};
use crate::ir::{TypeId, TypeKind};
use indexmap::IndexMap;
use std::collections::HashSet;

/// Result of type inference on a BuildGraph.
#[derive(Debug)]
pub struct TypeInferenceResult<'src> {
    /// All inferred type definitions (composite types only).
    pub type_defs: Vec<InferredTypeDef<'src>>,
    /// Mapping from definition name to its result TypeId.
    pub entrypoint_types: IndexMap<&'src str, TypeId>,
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

/// Inferred field information collected during traversal.
#[derive(Debug, Clone)]
struct FieldInfo {
    base_type: TypeId,
    cardinality: Cardinality,
    /// Number of branches this field appears in (for optional detection).
    branch_count: usize,
}

/// Collected scope information from traversal.
#[derive(Debug, Clone, Default)]
struct ScopeInfo<'src> {
    fields: IndexMap<&'src str, FieldInfo>,
    /// Variants for tagged alternations.
    variants: IndexMap<&'src str, ScopeInfo<'src>>,
    /// Whether we've seen variant markers (StartVariant/EndVariant).
    has_variants: bool,
}

impl<'src> ScopeInfo<'src> {
    fn add_field(&mut self, name: &'src str, base_type: TypeId, cardinality: Cardinality) {
        if let Some(existing) = self.fields.get_mut(name) {
            existing.cardinality = existing.cardinality.join(cardinality);
            existing.branch_count += 1;
        } else {
            self.fields.insert(
                name,
                FieldInfo {
                    base_type,
                    cardinality,
                    branch_count: 1,
                },
            );
        }
    }

    fn merge_from(&mut self, other: ScopeInfo<'src>, total_branches: usize) {
        for (name, info) in other.fields {
            if let Some(existing) = self.fields.get_mut(name) {
                existing.cardinality = existing.cardinality.join(info.cardinality);
                existing.branch_count += info.branch_count;
            } else {
                self.fields.insert(name, info);
            }
        }

        // Merge variants - don't overwrite, merge fields into existing
        for (tag, variant_info) in other.variants {
            if let Some(existing) = self.variants.get_mut(tag) {
                // Merge fields from child scope into existing variant
                for (name, info) in variant_info.fields {
                    if let Some(existing_field) = existing.fields.get_mut(name) {
                        existing_field.cardinality =
                            existing_field.cardinality.join(info.cardinality);
                        existing_field.branch_count += info.branch_count;
                    } else {
                        existing.fields.insert(name, info);
                    }
                }
            } else {
                self.variants.insert(tag, variant_info);
            }
            self.has_variants = true;
        }

        // Mark fields as optional if they don't appear in all branches
        for info in self.fields.values_mut() {
            if info.branch_count < total_branches {
                info.cardinality = info.cardinality.make_optional();
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
}

impl<'src, 'g> InferenceContext<'src, 'g> {
    fn new(graph: &'g BuildGraph<'src>, dead_nodes: &'g HashSet<NodeId>) -> Self {
        Self {
            graph,
            dead_nodes,
            type_defs: Vec::new(),
            next_type_id: 3, // TYPE_COMPOSITE_START
        }
    }

    fn alloc_type_id(&mut self) -> TypeId {
        let id = self.next_type_id;
        self.next_type_id += 1;
        id
    }

    fn infer_definition(&mut self, def_name: &'src str, entry_id: NodeId) -> TypeId {
        let mut visited = HashSet::new();
        let scope = self.traverse(entry_id, TraversalState::default(), &mut visited, 0);

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
                BuildEffect::Field(name) => {
                    if let Some(base_type) = state.pending_type.take() {
                        if let Some(tag) = state.current_variant {
                            // Inside a variant - add to variant scope
                            let variant_scope = scope.variants.entry(tag).or_default();
                            variant_scope.add_field(*name, base_type, state.cardinality);
                        } else {
                            scope.add_field(*name, base_type, state.cardinality);
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
            let child_scope = self.traverse(live_successors[0], state, visited, depth + 1);
            scope.merge_from(child_scope, 1);
        } else {
            // Branching - traverse each branch and merge results
            let total_branches = live_successors.len();
            for succ in live_successors {
                let child_scope = self.traverse(succ, state.clone(), visited, depth + 1);
                scope.merge_from(child_scope, total_branches);
            }
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
    }
}

/// Format inferred types for debugging/testing.
pub fn dump_types(result: &TypeInferenceResult) -> String {
    let mut out = String::new();

    out.push_str("=== Entrypoints ===\n");
    for (name, type_id) in &result.entrypoint_types {
        out.push_str(&format!("{} → {}\n", name, format_type_id(*type_id)));
    }

    if !result.type_defs.is_empty() {
        out.push_str("\n=== Types ===\n");
        for (idx, def) in result.type_defs.iter().enumerate() {
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

    out
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
