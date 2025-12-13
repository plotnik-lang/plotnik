//! Type inference for Query's BuildGraph.
//!
//! Analyzes the graph and infers output type structure for each definition.
//! Follows rules from ADR-0007 and ADR-0009.
//!
//! # Algorithm Overview
//!
//! 1. Pre-analyze array regions to detect QIS (Quantifier-Induced Scope)
//! 2. Walk graph from each definition entry point using stack-based scope tracking
//! 3. StartObject/EndObject delimit scopes that may become composite types
//! 4. QIS creates implicit structs when quantified expressions have ≥2 captures
//! 5. Field(name) consumes pending type and records it in current scope

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;
use rowan::TextRange;

use crate::diagnostics::{DiagnosticKind, Diagnostics};
use crate::ir::{TYPE_NODE, TYPE_STR, TYPE_VOID, TypeId, TypeKind};

use super::Query;
use super::graph::{BuildEffect, BuildGraph, NodeId};

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

/// Shape includes type information for proper compatibility checking.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TypeShape<'src> {
    Primitive(TypeId),
    Struct(Vec<(&'src str, TypeId)>),
    Composite(TypeId),
}

impl<'src> TypeShape<'src> {
    fn to_description(&self) -> TypeDescription {
        match self {
            TypeShape::Primitive(TYPE_NODE) => TypeDescription::Node,
            TypeShape::Primitive(TYPE_STR) => TypeDescription::String,
            TypeShape::Primitive(_) => TypeDescription::Node,
            TypeShape::Struct(fields) => {
                TypeDescription::Struct(fields.iter().map(|(n, _)| n.to_string()).collect())
            }
            TypeShape::Composite(_) => TypeDescription::Struct(vec!["...".to_string()]),
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
        shape: TypeShape<'src>,
        cardinality: Cardinality,
        span: TextRange,
    ) {
        if let Some(existing) = self.fields.get_mut(name) {
            existing.cardinality = existing.cardinality.join(cardinality);
            existing.branch_count += 1;
            if !existing.all_shapes.contains(&shape) {
                existing.all_shapes.push(shape);
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

    fn is_empty(&self) -> bool {
        self.fields.is_empty() && self.variants.is_empty()
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
        (TypeShape::Composite(t1), TypeShape::Composite(t2)) if t1 == t2 => None,
        (TypeShape::Struct(fields_a), TypeShape::Struct(fields_b)) => {
            if fields_a.len() != fields_b.len() {
                return Some(MergeError {
                    field,
                    shapes: vec![a.clone(), b.clone()],
                    spans: vec![],
                });
            }
            for ((name_a, type_a), (name_b, type_b)) in fields_a.iter().zip(fields_b.iter()) {
                if name_a != name_b || type_a != type_b {
                    return Some(MergeError {
                        field,
                        shapes: vec![a.clone(), b.clone()],
                        spans: vec![],
                    });
                }
            }
            None
        }
        _ => Some(MergeError {
            field,
            shapes: vec![a.clone(), b.clone()],
            spans: vec![],
        }),
    }
}

/// Entry on the scope stack during traversal.
#[derive(Debug, Clone)]
struct ScopeStackEntry<'src> {
    scope: ScopeInfo<'src>,
    is_object: bool,
    /// Captures pending type before StartObject (for sequences captured as a whole)
    outer_pending: Option<PendingType<'src>>,
}

impl<'src> ScopeStackEntry<'src> {
    fn new_root() -> Self {
        Self {
            scope: ScopeInfo::default(),
            is_object: false,
            outer_pending: None,
        }
    }

    fn new_object(outer_pending: Option<PendingType<'src>>) -> Self {
        Self {
            scope: ScopeInfo::default(),
            is_object: true,
            outer_pending,
        }
    }
}

/// Pending type waiting for a Field assignment.
#[derive(Debug, Clone)]
struct PendingType<'src> {
    shape: TypeShape<'src>,
    base_type: TypeId,
    cardinality: Cardinality,
}

impl<'src> PendingType<'src> {
    fn primitive(ty: TypeId) -> Self {
        Self {
            shape: TypeShape::Primitive(ty),
            base_type: ty,
            cardinality: Cardinality::One,
        }
    }
}

/// Pre-computed info about an array region for QIS detection.
#[derive(Debug, Clone)]
struct ArrayRegionInfo<'src> {
    /// Field names captured within this array region (excluding nested arrays)
    captures: Vec<(&'src str, TextRange)>,
    /// Whether QIS is triggered (≥2 captures)
    qis_triggered: bool,
}

/// Tracks state within a quantified (array) region.
#[derive(Debug, Clone)]
struct ArrayFrame<'src> {
    /// Node ID of the StartArray
    start_id: NodeId,
    /// Cardinality of this array (Star or Plus)
    cardinality: Cardinality,
    /// Pre-computed region info (captures, QIS status)
    region_info: ArrayRegionInfo<'src>,
}

#[derive(Debug, Clone)]
struct TraversalState<'src> {
    pending: Option<PendingType<'src>>,
    current_variant: Option<&'src str>,
    /// Stack of array frames (tracking array nesting)
    array_stack: Vec<ArrayFrame<'src>>,
    /// Set of fields that should be skipped (handled by QIS)
    skip_fields: HashSet<&'src str>,
}

impl<'src> Default for TraversalState<'src> {
    fn default() -> Self {
        Self {
            pending: None,
            current_variant: None,
            array_stack: Vec::new(),
            skip_fields: HashSet::new(),
        }
    }
}

impl<'src> TraversalState<'src> {
    fn current_array_cardinality(&self) -> Cardinality {
        self.array_stack
            .iter()
            .filter(|f| !f.region_info.qis_triggered)
            .fold(Cardinality::One, |acc, frame| {
                acc.multiply(frame.cardinality)
            })
    }
}

struct InferenceContext<'src, 'g> {
    graph: &'g BuildGraph<'src>,
    dead_nodes: &'g HashSet<NodeId>,
    type_defs: Vec<InferredTypeDef<'src>>,
    next_type_id: TypeId,
    diagnostics: Diagnostics,
    errors: Vec<UnificationError<'src>>,
    current_def_name: &'src str,
    /// Whether we're at definition root level (no fields assigned yet at root scope)
    at_definition_root: bool,
    /// Pre-computed array region info for QIS detection
    array_regions: HashMap<NodeId, ArrayRegionInfo<'src>>,
    /// Node ID of root-level QIS array (skip type creation in traverse for this)
    root_qis_node: Option<NodeId>,
}

impl<'src, 'g> InferenceContext<'src, 'g> {
    fn new(graph: &'g BuildGraph<'src>, dead_nodes: &'g HashSet<NodeId>) -> Self {
        let array_regions = analyze_array_regions(graph, dead_nodes);
        Self {
            graph,
            dead_nodes,
            type_defs: Vec::new(),
            next_type_id: 3, // TYPE_COMPOSITE_START
            diagnostics: Diagnostics::new(),
            errors: Vec::new(),
            current_def_name: "",
            at_definition_root: true,
            array_regions,
            root_qis_node: None,
        }
    }

    fn alloc_type_id(&mut self) -> TypeId {
        let id = self.next_type_id;
        self.next_type_id += 1;
        id
    }

    fn infer_definition(&mut self, def_name: &'src str, entry_id: NodeId) -> TypeId {
        self.current_def_name = def_name;
        self.at_definition_root = true;
        let mut visited = HashSet::new();
        let mut merge_errors = Vec::new();
        let mut scope_stack = vec![ScopeStackEntry::new_root()];

        // Check if definition starts with a QIS array (array at root with ≥2 captures)
        let root_qis_info = self.check_root_qis(entry_id);
        if root_qis_info.is_some() {
            self.root_qis_node = Some(entry_id);
        }

        self.traverse(
            entry_id,
            TraversalState::default(),
            &mut visited,
            0,
            &mut merge_errors,
            &mut scope_stack,
        );

        let root_entry = scope_stack
            .pop()
            .unwrap_or_else(|| ScopeStackEntry::new_root());
        let scope = root_entry.scope;

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

        // Check for QIS at definition root
        if let Some((captures, cardinality)) = root_qis_info {
            if !captures.is_empty() {
                let element_name = format!("{}Item", def_name);
                let element_name: &'src str = Box::leak(element_name.into_boxed_str());
                let element_type_id = self.create_qis_struct_type(element_name, &captures);
                return self.wrap_with_cardinality(element_type_id, cardinality);
            }
        }

        if scope.has_variants && !scope.variants.is_empty() {
            self.create_enum_type(def_name, &scope)
        } else if !scope.fields.is_empty() {
            self.create_struct_type(def_name, &scope)
        } else {
            TYPE_VOID
        }
    }

    /// Check if definition root has a QIS array (returns captures and cardinality if so).
    fn check_root_qis(
        &self,
        entry_id: NodeId,
    ) -> Option<(Vec<(&'src str, TextRange)>, Cardinality)> {
        let node = self.graph.node(entry_id);
        let has_start_array = node
            .effects
            .iter()
            .any(|e| matches!(e, BuildEffect::StartArray));

        if !has_start_array {
            return None;
        }

        let region_info = self.array_regions.get(&entry_id)?;
        if !region_info.qis_triggered {
            return None;
        }

        // TODO: Determine actual cardinality (Star vs Plus) from graph structure
        Some((region_info.captures.clone(), Cardinality::Star))
    }

    fn traverse(
        &mut self,
        node_id: NodeId,
        mut state: TraversalState<'src>,
        visited: &mut HashSet<NodeId>,
        depth: usize,
        errors: &mut Vec<MergeError<'src>>,
        scope_stack: &mut Vec<ScopeStackEntry<'src>>,
    ) {
        if self.dead_nodes.contains(&node_id) || depth > 200 {
            return;
        }

        if !visited.insert(node_id) {
            return;
        }

        let node = self.graph.node(node_id);

        for effect in &node.effects {
            match effect {
                BuildEffect::CaptureNode => {
                    state.pending = Some(PendingType::primitive(TYPE_NODE));
                }
                BuildEffect::ToString => {
                    state.pending = Some(PendingType::primitive(TYPE_STR));
                }
                BuildEffect::Field { name, span } => {
                    if let Some(pending) = state.pending.take() {
                        self.at_definition_root = false;

                        // Skip fields that are handled by QIS
                        if state.skip_fields.contains(name) {
                            continue;
                        }

                        let current_variant = state.current_variant;
                        let current_scope = scope_stack
                            .last_mut()
                            .map(|e| &mut e.scope)
                            .expect("scope stack should not be empty");

                        let effective_cardinality = pending
                            .cardinality
                            .multiply(state.current_array_cardinality());

                        if let Some(tag) = current_variant {
                            let variant_scope = current_scope.variants.entry(tag).or_default();
                            variant_scope.add_field(
                                *name,
                                pending.base_type,
                                pending.shape,
                                effective_cardinality,
                                *span,
                            );
                        } else {
                            current_scope.add_field(
                                *name,
                                pending.base_type,
                                pending.shape,
                                effective_cardinality,
                                *span,
                            );
                        }
                    }
                }
                BuildEffect::StartArray => {
                    // Look up pre-computed region info for this StartArray node
                    let region_info =
                        self.array_regions
                            .get(&node_id)
                            .cloned()
                            .unwrap_or_else(|| ArrayRegionInfo {
                                captures: Vec::new(),
                                qis_triggered: false,
                            });

                    // If QIS triggered, mark these fields to skip during traversal
                    if region_info.qis_triggered {
                        for (name, _) in &region_info.captures {
                            state.skip_fields.insert(*name);
                        }
                    }

                    state.array_stack.push(ArrayFrame {
                        start_id: node_id,
                        cardinality: Cardinality::Star,
                        region_info,
                    });
                }
                BuildEffect::PushElement => {}
                BuildEffect::EndArray => {
                    if let Some(array_frame) = state.array_stack.pop() {
                        let array_card = array_frame.cardinality;
                        let is_root_qis = self.root_qis_node == Some(array_frame.start_id);

                        // Remove skip_fields for this array's captures
                        if array_frame.region_info.qis_triggered {
                            for (name, _) in &array_frame.region_info.captures {
                                state.skip_fields.remove(name);
                            }

                            // Skip type creation for root-level QIS (handled in infer_definition)
                            if !is_root_qis {
                                // QIS: create element struct from pre-computed captures
                                let captures = &array_frame.region_info.captures;
                                if !captures.is_empty() {
                                    let element_name = self.generate_qis_element_name(None);
                                    let element_type_id =
                                        self.create_qis_struct_type(element_name, captures);
                                    let array_type_id =
                                        self.wrap_with_cardinality(element_type_id, array_card);

                                    state.pending = Some(PendingType {
                                        shape: TypeShape::Composite(array_type_id),
                                        base_type: array_type_id,
                                        cardinality: Cardinality::One,
                                    });
                                }
                            }
                        }
                        // Non-QIS arrays: fields were already added to parent scope
                        // with cardinality applied in the Field handler
                    }
                }
                BuildEffect::StartObject => {
                    let entry = ScopeStackEntry::new_object(state.pending.take());
                    scope_stack.push(entry);
                }
                BuildEffect::EndObject => {
                    if let Some(finished_entry) = scope_stack.pop() {
                        if finished_entry.is_object {
                            let finished_scope = finished_entry.scope;

                            if !finished_scope.is_empty() {
                                let type_name = self.generate_scope_name();
                                let type_id = self.create_struct_type(type_name, &finished_scope);

                                let field_types: Vec<(&'src str, TypeId)> = finished_scope
                                    .fields
                                    .iter()
                                    .map(|(name, info)| (*name, info.base_type))
                                    .collect();

                                state.pending = Some(PendingType {
                                    shape: TypeShape::Composite(type_id),
                                    base_type: type_id,
                                    cardinality: Cardinality::One,
                                });

                                if !field_types.is_empty() {
                                    if let Some(ref mut p) = state.pending {
                                        p.shape = TypeShape::Struct(field_types);
                                    }
                                }
                            } else {
                                state.pending = finished_entry.outer_pending;
                            }
                        } else {
                            scope_stack.push(finished_entry);
                        }
                    }
                }
                BuildEffect::StartVariant(tag) => {
                    state.current_variant = Some(*tag);
                    let current_scope = scope_stack
                        .last_mut()
                        .map(|e| &mut e.scope)
                        .expect("scope stack should not be empty");
                    current_scope.has_variants = true;
                }
                BuildEffect::EndVariant => {
                    if let Some(tag) = state.current_variant.take() {
                        let current_scope = scope_stack
                            .last_mut()
                            .map(|e| &mut e.scope)
                            .expect("scope stack should not be empty");
                        current_scope.variants.entry(tag).or_default();
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
            self.traverse(
                live_successors[0],
                state,
                visited,
                depth + 1,
                errors,
                scope_stack,
            );
        } else {
            // Branching: collect results from all branches, then merge
            let total_branches = live_successors.len();
            let initial_scope_len = scope_stack.len();
            let mut branch_scopes: Vec<ScopeInfo<'src>> = Vec::new();

            for succ in &live_successors {
                let mut branch_stack = scope_stack.clone();

                self.traverse(
                    *succ,
                    state.clone(),
                    &mut visited.clone(),
                    depth + 1,
                    errors,
                    &mut branch_stack,
                );

                while branch_stack.len() > initial_scope_len {
                    branch_stack.pop();
                }
                if let Some(entry) = branch_stack.last() {
                    branch_scopes.push(entry.scope.clone());
                }
            }

            if let Some(main_entry) = scope_stack.last_mut() {
                for branch_scope in branch_scopes {
                    let merge_errs = main_entry.scope.merge_from(branch_scope);
                    errors.extend(merge_errs);
                }
                main_entry.scope.apply_optionality(total_branches);
            }
        }
    }

    fn generate_scope_name(&self) -> &'src str {
        let name = format!("{}Scope{}", self.current_def_name, self.next_type_id);
        Box::leak(name.into_boxed_str())
    }

    fn generate_qis_element_name(&self, capture_name: Option<&'src str>) -> &'src str {
        let name = if let Some(cap) = capture_name {
            // Explicit capture: {Def}{Capture} with PascalCase
            let cap_pascal = to_pascal_case(cap);
            format!("{}{}", self.current_def_name, cap_pascal)
        } else if self.at_definition_root {
            // At definition root: {Def}Item
            format!("{}Item", self.current_def_name)
        } else {
            // Not at root and no capture - use synthetic name
            format!("{}Item{}", self.current_def_name, self.next_type_id)
        };
        Box::leak(name.into_boxed_str())
    }

    /// Create a struct type from QIS captures (all fields are Node type).
    fn create_qis_struct_type(
        &mut self,
        name: &'src str,
        captures: &[(&'src str, TextRange)],
    ) -> TypeId {
        let members: Vec<_> = captures
            .iter()
            .map(|(field_name, _span)| InferredMember {
                name: field_name,
                ty: TYPE_NODE, // QIS captures are always Node (could enhance later)
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

// ─────────────────────────────────────────────────────────────────────────────
// Array region analysis for QIS detection
// ─────────────────────────────────────────────────────────────────────────────

/// Pre-analyze all array regions to determine QIS triggering.
fn analyze_array_regions<'src>(
    graph: &BuildGraph<'src>,
    dead_nodes: &HashSet<NodeId>,
) -> HashMap<NodeId, ArrayRegionInfo<'src>> {
    let mut regions = HashMap::new();

    for (id, node) in graph.iter() {
        if dead_nodes.contains(&id) {
            continue;
        }
        let has_start_array = node
            .effects
            .iter()
            .any(|e| matches!(e, BuildEffect::StartArray));
        if has_start_array {
            let info = find_array_region_captures(graph, dead_nodes, id);
            regions.insert(id, info);
        }
    }

    regions
}

/// Find all captures within an array region (between StartArray and EndArray).
fn find_array_region_captures<'src>(
    graph: &BuildGraph<'src>,
    dead_nodes: &HashSet<NodeId>,
    start_id: NodeId,
) -> ArrayRegionInfo<'src> {
    let mut captures = Vec::new();
    let mut visited = HashSet::new();
    let mut stack = Vec::new();

    // Start from successors of the StartArray node
    let start_node = graph.node(start_id);
    for &succ in &start_node.successors {
        stack.push(succ);
    }

    while let Some(id) = stack.pop() {
        if dead_nodes.contains(&id) || !visited.insert(id) {
            continue;
        }

        let node = graph.node(id);

        // Check for EndArray - stop this path and record the ID
        let has_end_array = node
            .effects
            .iter()
            .any(|e| matches!(e, BuildEffect::EndArray));
        if has_end_array {
            continue;
        }

        // Check for nested StartArray - skip its contents
        let has_start_array = node
            .effects
            .iter()
            .any(|e| matches!(e, BuildEffect::StartArray));
        if has_start_array {
            continue;
        }

        // Collect Field captures with their spans
        for effect in &node.effects {
            if let BuildEffect::Field { name, span } = effect {
                if !captures.iter().any(|(n, _)| n == name) {
                    captures.push((*name, *span));
                }
            }
        }

        // Continue to successors
        for &succ in &node.successors {
            stack.push(succ);
        }
    }

    let qis_triggered = captures.len() >= 2;
    ArrayRegionInfo {
        captures,
        qis_triggered,
    }
}

fn to_pascal_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = true;
    for c in s.chars() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(c.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }
    result
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
