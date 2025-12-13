//! Type inference for Query's BuildGraph.
//!
//! Analyzes the graph structure statically to determine output types.
//! Follows rules from ADR-0006, ADR-0007 and ADR-0009.
//!
//! # Algorithm Overview
//!
//! 1. Traverse graph to collect all scope boundaries (StartObject/EndObject, StartArray/EndArray)
//! 2. Associate Field effects with their containing object scope
//! 3. Build types bottom-up from scope hierarchy
//! 4. Handle branching by merging fields with optionality rules

use std::collections::{HashMap, HashSet, VecDeque};

use indexmap::IndexMap;
use rowan::TextRange;

use crate::diagnostics::{DiagnosticKind, Diagnostics};
use crate::ir::{TYPE_NODE, TYPE_STR, TYPE_VOID, TypeId, TypeKind};

use super::Query;
use super::graph::{BuildEffect, BuildGraph, BuildNode, NodeId, RefMarker};

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

// ─────────────────────────────────────────────────────────────────────────────
// Cardinality
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Cardinality {
    #[default]
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

// ─────────────────────────────────────────────────────────────────────────────
// Field and Scope tracking
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
enum TypeShape {
    Primitive(TypeId),
}

impl TypeShape {
    fn to_description(&self) -> TypeDescription {
        match self {
            TypeShape::Primitive(TYPE_NODE) => TypeDescription::Node,
            TypeShape::Primitive(TYPE_STR) => TypeDescription::String,
            TypeShape::Primitive(_) => TypeDescription::Node,
        }
    }
}

#[derive(Debug, Clone)]
struct FieldInfo {
    base_type: TypeId,
    shape: TypeShape,
    cardinality: Cardinality,
    branch_count: usize,
    spans: Vec<TextRange>,
    is_array_type: bool,
}

#[derive(Debug, Clone, Default)]
struct ScopeInfo<'src> {
    fields: IndexMap<&'src str, FieldInfo>,
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
        is_array_type: bool,
    ) {
        let shape = TypeShape::Primitive(base_type);
        if let Some(existing) = self.fields.get_mut(name) {
            existing.cardinality = existing.cardinality.join(cardinality);
            existing.branch_count += 1;
            existing.spans.push(span);
            existing.is_array_type = existing.is_array_type || is_array_type;
        } else {
            self.fields.insert(
                name,
                FieldInfo {
                    base_type,
                    shape,
                    cardinality,
                    branch_count: 1,
                    spans: vec![span],
                    is_array_type,
                },
            );
        }
    }

    fn merge_from(&mut self, other: ScopeInfo<'src>) -> Vec<MergeError<'src>> {
        let mut errors = Vec::new();

        for (name, other_info) in other.fields {
            if let Some(existing) = self.fields.get_mut(name) {
                // Check type compatibility
                if existing.shape != other_info.shape {
                    errors.push(MergeError {
                        field: name,
                        shapes: vec![existing.shape.clone(), other_info.shape.clone()],
                        spans: existing
                            .spans
                            .iter()
                            .chain(&other_info.spans)
                            .cloned()
                            .collect(),
                    });
                }
                existing.cardinality = existing.cardinality.join(other_info.cardinality);
                existing.branch_count += other_info.branch_count;
                existing.spans.extend(other_info.spans);
            } else {
                self.fields.insert(name, other_info);
            }
        }

        for (tag, other_variant) in other.variants {
            let variant = self.variants.entry(tag).or_default();
            errors.extend(variant.merge_from(other_variant));
        }

        if other.has_variants {
            self.has_variants = true;
        }

        errors
    }

    fn apply_optionality(&mut self, total_branches: usize) {
        for info in self.fields.values_mut() {
            // Skip optionality for array-typed fields: arrays already encode
            // zero-or-more semantics, so Optional wrapper would be redundant
            if info.branch_count < total_branches && !info.is_array_type {
                info.cardinality = info.cardinality.make_optional();
            }
        }
    }

    fn is_empty(&self) -> bool {
        self.fields.is_empty() && self.variants.is_empty()
    }
}

#[derive(Debug)]
struct MergeError<'src> {
    field: &'src str,
    shapes: Vec<TypeShape>,
    spans: Vec<TextRange>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Scope stack for traversal
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct ScopeStackEntry<'src> {
    scope: ScopeInfo<'src>,
    is_object: bool,
    outer_pending: Option<PendingType>,
}

impl<'src> ScopeStackEntry<'src> {
    fn new_root() -> Self {
        Self {
            scope: ScopeInfo::default(),
            is_object: false,
            outer_pending: None,
        }
    }

    fn new_object(outer_pending: Option<PendingType>) -> Self {
        Self {
            scope: ScopeInfo::default(),
            is_object: true,
            outer_pending,
        }
    }
}

#[derive(Debug, Clone)]
struct PendingType {
    base_type: TypeId,
    cardinality: Cardinality,
    is_array: bool,
}

impl PendingType {
    fn primitive(base_type: TypeId) -> Self {
        Self {
            base_type,
            cardinality: Cardinality::One,
            is_array: false,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Traversal state
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Default)]
struct ArrayFrame {
    cardinality: Cardinality,
    element_type: Option<TypeId>,
    /// Node ID where this array started (for lookup in precomputed map)
    start_node: Option<NodeId>,
    /// Whether PushElement was actually called (vs prepass placeholder)
    push_called: bool,
}

#[derive(Clone, Default)]
struct TraversalState {
    pending: Option<PendingType>,
    current_variant: Option<&'static str>,
    array_stack: Vec<ArrayFrame>,
    object_depth: usize,
    /// When true, skip EndObject type creation.
    /// Used during alternation branch exploration to collect variants before creating enum.
    dry_run: bool,
}

impl TraversalState {
    fn effective_array_cardinality(&self) -> Cardinality {
        // Inside object scope, array cardinality doesn't apply to fields
        if self.object_depth > 0 {
            return Cardinality::One;
        }
        self.array_stack
            .iter()
            .fold(Cardinality::One, |acc, frame| {
                acc.multiply(frame.cardinality)
            })
    }
}

impl Cardinality {
    fn multiply(self, other: Cardinality) -> Cardinality {
        use Cardinality::*;
        match (self, other) {
            (One, x) | (x, One) => x,
            (Optional, Optional) => Optional,
            (Optional, Plus) | (Plus, Optional) => Star,
            (Optional, Star) | (Star, Optional) => Star,
            (Star, _) | (_, Star) => Star,
            (Plus, Plus) => Plus,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Inference context
// ─────────────────────────────────────────────────────────────────────────────

struct InferenceContext<'src, 'g> {
    graph: &'g BuildGraph<'src>,
    dead_nodes: &'g HashSet<NodeId>,
    type_defs: Vec<InferredTypeDef<'src>>,
    next_type_id: TypeId,
    diagnostics: Diagnostics,
    errors: Vec<UnificationError<'src>>,
    current_def_name: &'src str,
    /// Shared map for array element types across branches in loops.
    array_element_types: HashMap<NodeId, TypeId>,
    /// Map from definition name to its computed type (for reference lookups).
    definition_types: HashMap<&'src str, TypeId>,
}

impl<'src, 'g> InferenceContext<'src, 'g> {
    fn new(graph: &'g BuildGraph<'src>, dead_nodes: &'g HashSet<NodeId>) -> Self {
        Self {
            graph,
            dead_nodes,
            type_defs: Vec::new(),
            next_type_id: 3, // 0=void, 1=node, 2=str
            diagnostics: Diagnostics::default(),
            errors: Vec::new(),
            current_def_name: "",
            array_element_types: HashMap::new(),
            definition_types: HashMap::new(),
        }
    }

    fn alloc_type_id(&mut self) -> TypeId {
        let id = self.next_type_id;
        self.next_type_id += 1;
        id
    }

    fn infer_definition(&mut self, def_name: &'src str, entry_id: NodeId) -> TypeId {
        self.current_def_name = def_name;
        let mut visited = HashSet::new();
        let mut merge_errors = Vec::new();
        let mut scope_stack = vec![ScopeStackEntry::new_root()];

        let (final_pending, _) = self.traverse(
            entry_id,
            TraversalState::default(),
            &mut visited,
            0,
            &mut merge_errors,
            &mut scope_stack,
        );

        let root_entry = scope_stack.pop().unwrap_or_else(ScopeStackEntry::new_root);
        let scope = root_entry.scope;

        // Report merge errors
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

        // Determine result type
        if scope.has_variants && !scope.variants.is_empty() {
            self.create_enum_type(def_name, &scope)
        } else if !scope.fields.is_empty() {
            self.create_struct_type(def_name, &scope)
        } else if let Some(pending) = final_pending {
            pending.base_type
        } else {
            TYPE_VOID
        }
    }

    /// Returns (pending_type, stopped_at_node) where stopped_at_node is Some if
    /// traversal stopped at an already-visited node (reconvergence point).
    fn traverse(
        &mut self,
        node_id: NodeId,
        mut state: TraversalState,
        visited: &mut HashSet<NodeId>,
        depth: usize,
        errors: &mut Vec<MergeError<'src>>,
        scope_stack: &mut Vec<ScopeStackEntry<'src>>,
    ) -> (Option<PendingType>, Option<NodeId>) {
        if self.dead_nodes.contains(&node_id) || depth > 200 {
            return (state.pending, None);
        }

        if !visited.insert(node_id) {
            return (state.pending, Some(node_id));
        }

        let node = self.graph.node(node_id);

        for effect in &node.effects {
            self.process_effect(effect, node_id, &node.ref_marker, &mut state, scope_stack);
        }

        // Process successors
        let live_successors = self.get_live_successors(node);
        if live_successors.is_empty() {
            return (state.pending, None);
        }

        if live_successors.len() == 1 {
            return self.traverse(
                live_successors[0],
                state,
                visited,
                depth + 1,
                errors,
                scope_stack,
            );
        }

        self.explore_branches(live_successors, state, visited, depth, errors, scope_stack)
    }

    /// Process a single effect, updating state and scope_stack.
    fn process_effect(
        &mut self,
        effect: &BuildEffect<'src>,
        node_id: NodeId,
        ref_marker: &RefMarker,
        state: &mut TraversalState,
        scope_stack: &mut Vec<ScopeStackEntry<'src>>,
    ) {
        match effect {
            BuildEffect::CaptureNode => {
                let capture_type = match ref_marker {
                    RefMarker::Exit { ref_id } => self.find_ref_type(*ref_id).unwrap_or(TYPE_NODE),
                    _ => TYPE_NODE,
                };
                state.pending = Some(PendingType::primitive(capture_type));
            }
            BuildEffect::ClearCurrent => {
                state.pending = None;
            }
            BuildEffect::ToString => {
                state.pending = Some(PendingType::primitive(TYPE_STR));
            }
            BuildEffect::Field { name, span } => {
                self.process_field_effect(name, *span, state, scope_stack);
            }
            BuildEffect::StartArray { is_plus } => {
                let cardinality = if *is_plus {
                    Cardinality::Plus
                } else {
                    Cardinality::Star
                };
                state.array_stack.push(ArrayFrame {
                    cardinality,
                    element_type: None,
                    start_node: Some(node_id),
                    push_called: false,
                });
            }
            BuildEffect::PushElement => {
                self.process_push_element(state);
            }
            BuildEffect::EndArray => {
                self.process_end_array(state);
            }
            BuildEffect::StartObject => {
                state.object_depth += 1;
                scope_stack.push(ScopeStackEntry::new_object(state.pending.take()));
            }
            BuildEffect::EndObject => {
                state.object_depth = state.object_depth.saturating_sub(1);
                if !state.dry_run {
                    self.process_end_object(state, scope_stack);
                }
            }
            BuildEffect::StartVariant(tag) => {
                // SAFETY: tag comes from source with 'src lifetime
                let tag: &'static str = unsafe { std::mem::transmute(*tag) };
                state.current_variant = Some(tag);
                if let Some(entry) = scope_stack.last_mut() {
                    entry.scope.has_variants = true;
                }
            }
            BuildEffect::EndVariant => {
                self.process_end_variant(state, scope_stack);
            }
        }
    }

    fn process_field_effect(
        &self,
        name: &str,
        span: TextRange,
        state: &mut TraversalState,
        scope_stack: &mut Vec<ScopeStackEntry<'src>>,
    ) {
        let Some(pending) = state.pending.take() else {
            return;
        };

        // SAFETY: name comes from source with 'src lifetime
        let name: &'src str = unsafe { std::mem::transmute(name) };
        let current_variant: Option<&'src str> = state
            .current_variant
            .map(|v| unsafe { std::mem::transmute(v) });

        let effective_card = pending
            .cardinality
            .multiply(state.effective_array_cardinality());
        let Some(entry) = scope_stack.last_mut() else {
            return;
        };

        // Inside object scope, fields go to object, not variant scope
        let target_scope = match current_variant.filter(|_| state.object_depth == 0) {
            Some(tag) => entry.scope.variants.entry(tag).or_default(),
            None => &mut entry.scope,
        };
        target_scope.add_field(
            name,
            pending.base_type,
            effective_card,
            span,
            pending.is_array,
        );
    }

    fn process_push_element(&mut self, state: &mut TraversalState) {
        let Some(pending) = state.pending.take() else {
            return;
        };
        let Some(frame) = state.array_stack.last_mut() else {
            return;
        };

        frame.element_type = Some(pending.base_type);
        frame.push_called = true;
        if let Some(start_id) = frame.start_node {
            self.array_element_types.insert(start_id, pending.base_type);
        }
    }

    fn process_end_array(&mut self, state: &mut TraversalState) {
        let Some(frame) = state.array_stack.pop() else {
            return;
        };

        let push_was_called = frame.push_called
            || frame
                .start_node
                .map_or(false, |id| self.array_element_types.contains_key(&id));

        if !push_was_called {
            return;
        }

        let element_type = frame
            .start_node
            .and_then(|id| self.array_element_types.get(&id).copied())
            .or(frame.element_type)
            .unwrap_or(TYPE_NODE);

        let array_type = self.wrap_with_cardinality(element_type, frame.cardinality);
        state.pending = Some(PendingType {
            base_type: array_type,
            cardinality: Cardinality::One,
            is_array: true,
        });
    }

    fn process_end_object(
        &mut self,
        state: &mut TraversalState,
        scope_stack: &mut Vec<ScopeStackEntry<'src>>,
    ) {
        let Some(finished_entry) = scope_stack.pop() else {
            return;
        };
        if !finished_entry.is_object {
            scope_stack.push(finished_entry);
            return;
        }

        let finished_scope = finished_entry.scope;
        if finished_scope.is_empty() {
            state.pending = finished_entry.outer_pending;
            return;
        }

        let type_name = self.generate_scope_name();
        let type_id = if finished_scope.has_variants && !finished_scope.variants.is_empty() {
            self.create_enum_type(type_name, &finished_scope)
        } else {
            self.create_struct_type(type_name, &finished_scope)
        };

        state.pending = Some(PendingType {
            base_type: type_id,
            cardinality: Cardinality::One,
            is_array: false,
        });
    }

    fn process_end_variant(
        &self,
        state: &mut TraversalState,
        scope_stack: &mut Vec<ScopeStackEntry<'src>>,
    ) {
        let Some(tag) = state.current_variant.take() else {
            return;
        };
        // SAFETY: tag comes from source with 'src lifetime
        let tag: &'src str = unsafe { std::mem::transmute(tag) };

        let Some(entry) = scope_stack.last_mut() else {
            return;
        };
        let variant_scope = entry.scope.variants.entry(tag).or_default();

        // Single-capture flattening (ADR-0007)
        if variant_scope.fields.is_empty() {
            if let Some(pending) = state.pending.take() {
                variant_scope.add_field(
                    "$value",
                    pending.base_type,
                    pending.cardinality,
                    rowan::TextRange::default(),
                    pending.is_array,
                );
            }
        }
    }

    /// Get live successors, filtering dead nodes and ref entry points.
    fn get_live_successors(&self, node: &BuildNode<'src>) -> Vec<NodeId> {
        let def_entry_to_skip = match &node.ref_marker {
            RefMarker::Enter { .. } => node.ref_name.and_then(|name| self.graph.definition(name)),
            _ => None,
        };

        node.successors
            .iter()
            .copied()
            .filter(|s| !self.dead_nodes.contains(s))
            .filter(|s| def_entry_to_skip.map_or(true, |def| *s != def))
            .collect()
    }

    /// Explore multiple branches, merge scopes, handle reconvergence.
    fn explore_branches(
        &mut self,
        successors: Vec<NodeId>,
        state: TraversalState,
        visited: &mut HashSet<NodeId>,
        depth: usize,
        errors: &mut Vec<MergeError<'src>>,
        scope_stack: &mut Vec<ScopeStackEntry<'src>>,
    ) -> (Option<PendingType>, Option<NodeId>) {
        let total_branches = successors.len();
        let initial_scope_len = scope_stack.len();
        let use_dry_run = state.object_depth > 0;

        let mut branch_scopes: Vec<ScopeInfo<'src>> = Vec::new();
        let mut branch_visited_sets: Vec<HashSet<NodeId>> = Vec::new();
        let mut result_pending: Option<PendingType> = None;

        // Phase 1: explore branches independently
        for succ in &successors {
            let mut branch_stack = scope_stack.clone();
            let mut branch_visited = visited.clone();
            let mut branch_state = state.clone();
            branch_state.dry_run = use_dry_run;

            let (branch_pending, _) = self.traverse(
                *succ,
                branch_state,
                &mut branch_visited,
                depth + 1,
                errors,
                &mut branch_stack,
            );

            if result_pending.is_none() {
                result_pending = branch_pending;
            }

            let new_nodes: HashSet<NodeId> = branch_visited.difference(visited).copied().collect();
            branch_visited_sets.push(new_nodes);

            while branch_stack.len() > initial_scope_len {
                branch_stack.pop();
            }
            if let Some(entry) = branch_stack.last() {
                branch_scopes.push(entry.scope.clone());
            }
        }

        // Merge branch scopes
        if let Some(main_entry) = scope_stack.last_mut() {
            for branch_scope in branch_scopes {
                errors.extend(main_entry.scope.merge_from(branch_scope));
            }
            main_entry.scope.apply_optionality(total_branches);
        }

        // Find and process reconvergence
        let reconverge_nodes = self.find_reconvergence(&branch_visited_sets);

        if use_dry_run && !reconverge_nodes.is_empty() {
            if let Some(entry_node) = reconverge_nodes.iter().min().copied() {
                for branch_set in &branch_visited_sets {
                    for &nid in branch_set {
                        if !reconverge_nodes.contains(&nid) {
                            visited.insert(nid);
                        }
                    }
                }
                let mut cont_state = state;
                cont_state.dry_run = false;
                cont_state.pending = result_pending;
                return self.traverse(
                    entry_node,
                    cont_state,
                    visited,
                    depth + 1,
                    errors,
                    scope_stack,
                );
            }
        }

        for branch_set in branch_visited_sets {
            visited.extend(branch_set);
        }
        (result_pending, None)
    }

    fn find_reconvergence(&self, branch_sets: &[HashSet<NodeId>]) -> HashSet<NodeId> {
        if branch_sets.len() < 2 {
            return HashSet::new();
        }
        let mut iter = branch_sets.iter();
        let first = iter.next().unwrap().clone();
        iter.fold(first, |acc, set| acc.intersection(set).copied().collect())
    }

    fn generate_scope_name(&self) -> &'src str {
        let name = format!("{}Scope{}", self.current_def_name, self.next_type_id);
        Box::leak(name.into_boxed_str())
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
            } else if variant_scope.fields.len() == 1 {
                // Single-capture variant: flatten to capture's type directly (ADR-0007)
                let (_, info) = variant_scope.fields.iter().next().unwrap();
                self.wrap_with_cardinality(info.base_type, info.cardinality)
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

    /// Find the type for a reference by looking up the Enter node with matching ref_id.
    fn find_ref_type(&self, ref_id: u32) -> Option<TypeId> {
        // Find the Enter node with this ref_id to get the definition name
        for (_, node) in self.graph.iter() {
            if let RefMarker::Enter { ref_id: enter_id } = &node.ref_marker {
                if *enter_id == ref_id {
                    if let Some(name) = node.ref_name {
                        return self.definition_types.get(name).copied();
                    }
                }
            }
        }
        None
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

// ─────────────────────────────────────────────────────────────────────────────
// Query integration
// ─────────────────────────────────────────────────────────────────────────────

impl<'a> Query<'a> {
    /// Run type inference on the built graph.
    pub(super) fn infer_types(&mut self) {
        let mut ctx = InferenceContext::new(&self.graph, &self.dead_nodes);

        // Process definitions in dependency order (referenced definitions first)
        let sorted = self.topological_sort_definitions();
        for name in sorted {
            if let Some(entry_id) = self.graph.definition(name) {
                let type_id = ctx.infer_definition(name, entry_id);
                ctx.definition_types.insert(name, type_id);
                self.type_info.entrypoint_types.insert(name, type_id);
            }
        }

        self.type_info.type_defs = ctx.type_defs;
        self.type_info.diagnostics = ctx.diagnostics;
        self.type_info.errors = ctx.errors;
    }

    /// Topologically sort definitions so referenced definitions are processed first.
    fn topological_sort_definitions(&self) -> Vec<&'a str> {
        let definitions: Vec<_> = self.graph.definitions().collect();
        let def_names: HashSet<&str> = definitions.iter().map(|(name, _)| *name).collect();

        // Build dependency graph: which definitions does each definition reference?
        let mut deps: HashMap<&str, Vec<&str>> = HashMap::new();
        for &(name, entry_id) in &definitions {
            let refs = self.collect_references(entry_id, &def_names);
            deps.insert(name, refs);
        }

        // Kahn's algorithm for topological sort
        let mut in_degree: HashMap<&str, usize> = HashMap::new();
        for &(name, _) in &definitions {
            in_degree.insert(name, 0);
        }
        for refs in deps.values() {
            for &dep in refs {
                *in_degree.entry(dep).or_insert(0) += 1;
            }
        }

        let mut zero_degree: Vec<&str> = in_degree
            .iter()
            .filter(|(_, deg)| **deg == 0)
            .map(|(&name, _)| name)
            .collect();
        zero_degree.sort();
        let mut queue: VecDeque<&str> = zero_degree.into_iter().collect();

        let mut sorted = Vec::new();
        while let Some(name) = queue.pop_front() {
            sorted.push(name);
            if let Some(refs) = deps.get(name) {
                for &dep in refs {
                    if let Some(deg) = in_degree.get_mut(dep) {
                        *deg = deg.saturating_sub(1);
                        if *deg == 0 {
                            queue.push_back(dep);
                        }
                    }
                }
            }
        }

        // Reverse so dependencies come first
        sorted.reverse();

        // Add any remaining (cyclic) definitions
        for &(name, _) in &definitions {
            if !sorted.contains(&name) {
                sorted.push(name);
            }
        }

        sorted
    }

    /// Collect all definition names referenced from a given node.
    fn collect_references(&self, start: NodeId, def_names: &HashSet<&str>) -> Vec<&'a str> {
        let mut refs = Vec::new();
        let mut visited = HashSet::new();
        let mut stack = vec![start];

        while let Some(node_id) = stack.pop() {
            if !visited.insert(node_id) {
                continue;
            }
            let node = self.graph.node(node_id);

            // Check if this is an Enter node referencing another definition
            if let RefMarker::Enter { .. } = &node.ref_marker {
                if let Some(name) = node.ref_name {
                    if def_names.contains(name) && !refs.contains(&name) {
                        refs.push(name);
                    }
                }
            }

            // Don't follow into referenced definitions (they're opaque)
            let skip_def = match &node.ref_marker {
                RefMarker::Enter { .. } => node.ref_name.and_then(|n| self.graph.definition(n)),
                _ => None,
            };

            for &succ in &node.successors {
                if skip_def.map_or(true, |def| succ != def) {
                    stack.push(succ);
                }
            }
        }

        refs
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Display and helpers
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
                let type_id = 3 + idx as TypeId;
                let name = def.name.unwrap_or("<anon>");
                match def.kind {
                    TypeKind::Record => {
                        out.push_str(&format!("T{}: Record {} {{\n", type_id, name));
                        for member in &def.members {
                            out.push_str(&format!(
                                "    {}: {}\n",
                                member.name,
                                format_type_id(member.ty)
                            ));
                        }
                        out.push_str("}\n");
                    }
                    TypeKind::Enum => {
                        out.push_str(&format!("T{}: Enum {} {{\n", type_id, name));
                        for member in &def.members {
                            out.push_str(&format!(
                                "    {}: {}\n",
                                member.name,
                                format_type_id(member.ty)
                            ));
                        }
                        out.push_str("}\n");
                    }
                    TypeKind::Optional => {
                        let inner = def.inner_type.map(format_type_id).unwrap_or_default();
                        out.push_str(&format!("T{}: Optional {} → {}\n", type_id, name, inner));
                    }
                    TypeKind::ArrayStar => {
                        let inner = def.inner_type.map(format_type_id).unwrap_or_default();
                        out.push_str(&format!("T{}: ArrayStar {} → {}\n", type_id, name, inner));
                    }
                    TypeKind::ArrayPlus => {
                        let inner = def.inner_type.map(format_type_id).unwrap_or_default();
                        out.push_str(&format!("T{}: ArrayPlus {} → {}\n", type_id, name, inner));
                    }
                }
            }
        }

        if !self.errors.is_empty() {
            out.push_str("\n=== Errors ===\n");
            for err in &self.errors {
                let types = err
                    .types_found
                    .iter()
                    .map(|t| t.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push_str(&format!(
                    "field `{}` in `{}`: incompatible types [{}]\n",
                    err.field, err.definition, types
                ));
            }
        }

        out
    }

    pub fn dump_diagnostics(&self, source: &str) -> String {
        self.diagnostics.render_filtered(source)
    }

    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

fn format_type_id(id: TypeId) -> String {
    match id {
        TYPE_VOID => "Void".to_string(),
        TYPE_NODE => "Node".to_string(),
        TYPE_STR => "String".to_string(),
        _ => format!("T{}", id),
    }
}
