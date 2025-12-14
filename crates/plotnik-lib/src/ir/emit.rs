//! Query emitter: transforms BuildGraph + TypeInferenceResult into CompiledQuery.
//!
//! Three-pass construction:
//! 1. Analysis: count elements, intern strings, collect data
//! 2. Layout: compute aligned offsets, allocate once
//! 3. Emission: write via ptr::write

use std::collections::HashMap;
use std::ptr;

use super::compiled::{CompiledQuery, CompiledQueryBuffer, align_up};
use super::ids::{NodeFieldId, NodeTypeId, RefId, StringId, TYPE_NODE, TransitionId};
use super::strings::StringInterner;
use super::{
    EffectOp, Entrypoint, MAX_INLINE_SUCCESSORS, Matcher, RefTransition, Slice, StringRef,
    Transition, TypeDef, TypeMember,
};

use crate::query::graph::{BuildEffect, BuildGraph, BuildMatcher, BuildNode, RefMarker};
use crate::query::infer::TypeInferenceResult;

/// Callback for resolving node kind names to IDs.
pub trait NodeKindResolver {
    /// Resolves a named node kind to its ID. Returns `None` if unknown.
    fn resolve_kind(&self, name: &str) -> Option<NodeTypeId>;

    /// Resolves a field name to its ID. Returns `None` if unknown.
    fn resolve_field(&self, name: &str) -> Option<NodeFieldId>;
}

/// A resolver that always fails (for testing without tree-sitter).
pub struct NullResolver;

impl NodeKindResolver for NullResolver {
    fn resolve_kind(&self, _name: &str) -> Option<NodeTypeId> {
        None
    }
    fn resolve_field(&self, _name: &str) -> Option<NodeFieldId> {
        None
    }
}

/// Map-based resolver for testing.
pub struct MapResolver {
    kinds: HashMap<String, NodeTypeId>,
    fields: HashMap<String, NodeFieldId>,
}

impl MapResolver {
    pub fn new() -> Self {
        Self {
            kinds: HashMap::new(),
            fields: HashMap::new(),
        }
    }

    pub fn add_kind(&mut self, name: impl Into<String>, id: NodeTypeId) {
        self.kinds.insert(name.into(), id);
    }

    pub fn add_field(&mut self, name: impl Into<String>, id: NodeFieldId) {
        self.fields.insert(name.into(), id);
    }
}

impl Default for MapResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl NodeKindResolver for MapResolver {
    fn resolve_kind(&self, name: &str) -> Option<NodeTypeId> {
        self.kinds.get(name).copied()
    }

    fn resolve_field(&self, name: &str) -> Option<NodeFieldId> {
        self.fields.get(name).copied()
    }
}

/// Query emitter error.
#[derive(Debug, Clone)]
pub enum EmitError {
    /// Unknown node kind encountered.
    UnknownNodeKind(String),
    /// Unknown field name encountered.
    UnknownField(String),
    /// Too many transitions (exceeds u32::MAX).
    TooManyTransitions,
    /// Too many successors (exceeds u32::MAX).
    TooManySuccessors,
    /// Too many effects (exceeds u32::MAX).
    TooManyEffects,
    /// Internal consistency error.
    InternalError(String),
}

impl std::fmt::Display for EmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EmitError::UnknownNodeKind(s) => write!(f, "unknown node kind: {}", s),
            EmitError::UnknownField(s) => write!(f, "unknown field: {}", s),
            EmitError::TooManyTransitions => write!(f, "too many transitions"),
            EmitError::TooManySuccessors => write!(f, "too many successors"),
            EmitError::TooManyEffects => write!(f, "too many effects"),
            EmitError::InternalError(s) => write!(f, "internal error: {}", s),
        }
    }
}

impl std::error::Error for EmitError {}

/// Result type for emit operations.
pub type EmitResult<T> = Result<T, EmitError>;

/// Emitter state during analysis phase.
struct EmitContext<'src, 'g> {
    graph: &'g BuildGraph<'src>,
    type_info: &'g TypeInferenceResult<'src>,
    strings: StringInterner<'src>,

    // Collected data
    effects: Vec<EffectOp>,
    negated_fields: Vec<NodeFieldId>,
    /// Spilled successors (for transitions with >8 successors)
    spilled_successors: Vec<TransitionId>,

    // Maps from BuildGraph to IR
    /// For each transition, its effects slice
    transition_effects: Vec<Slice<EffectOp>>,
    /// For each transition, its negated fields slice
    transition_negated_fields: Vec<Slice<NodeFieldId>>,
    /// For each transition, if successors spill: (start_index in spilled_successors, count)
    transition_spilled: Vec<Option<(u32, u32)>>,
}

impl<'src, 'g> EmitContext<'src, 'g> {
    fn new(graph: &'g BuildGraph<'src>, type_info: &'g TypeInferenceResult<'src>) -> Self {
        let node_count = graph.len();
        Self {
            graph,
            type_info,
            strings: StringInterner::new(),
            effects: Vec::new(),
            negated_fields: Vec::new(),
            spilled_successors: Vec::new(),
            transition_effects: Vec::with_capacity(node_count),
            transition_negated_fields: Vec::with_capacity(node_count),
            transition_spilled: Vec::with_capacity(node_count),
        }
    }

    fn intern(&mut self, s: &'src str) -> StringId {
        self.strings.intern(s)
    }
}

/// Layout information computed in pass 2.
struct LayoutInfo {
    buffer_len: usize,
    successors_offset: u32,
    effects_offset: u32,
    negated_fields_offset: u32,
    string_refs_offset: u32,
    string_bytes_offset: u32,
    type_defs_offset: u32,
    type_members_offset: u32,
    entrypoints_offset: u32,
    trivia_kinds_offset: u32,

    // Counts
    transition_count: u32,
    successor_count: u32,
    effect_count: u32,
    negated_field_count: u16,
    string_ref_count: u16,
    type_def_count: u16,
    type_member_count: u16,
    entrypoint_count: u16,
    trivia_kind_count: u16,
}

/// Emits a compiled query from a BuildGraph.
pub struct QueryEmitter<'src, 'g, R> {
    ctx: EmitContext<'src, 'g>,
    resolver: R,
    trivia_kinds: Vec<NodeTypeId>,
}

impl<'src, 'g, R: NodeKindResolver> QueryEmitter<'src, 'g, R> {
    /// Creates a new emitter.
    pub fn new(
        graph: &'g BuildGraph<'src>,
        type_info: &'g TypeInferenceResult<'src>,
        resolver: R,
    ) -> Self {
        Self {
            ctx: EmitContext::new(graph, type_info),
            resolver,
            trivia_kinds: Vec::new(),
        }
    }

    /// Sets trivia node kinds (e.g., comments) to skip during execution.
    pub fn with_trivia_kinds(mut self, kinds: Vec<NodeTypeId>) -> Self {
        self.trivia_kinds = kinds;
        self
    }

    /// Emits the compiled query.
    pub fn emit(mut self) -> EmitResult<CompiledQuery> {
        // Pass 1: Analysis
        self.analyze()?;

        // Pass 2: Layout
        let layout = self.compute_layout()?;

        // Pass 3: Emission
        self.emit_buffer(layout)
    }

    fn analyze(&mut self) -> EmitResult<()> {
        // Pre-intern definition names for entrypoints
        for (name, _) in self.ctx.graph.definitions() {
            self.ctx.intern(name);
        }

        // Pre-intern type names
        for type_def in &self.ctx.type_info.type_defs {
            if let Some(name) = type_def.name {
                self.ctx.intern(name);
            }
            for member in &type_def.members {
                self.ctx.intern(member.name);
            }
        }

        // Analyze each transition
        for (_, node) in self.ctx.graph.iter() {
            self.analyze_node(node)?;
        }

        Ok(())
    }

    fn analyze_node(&mut self, node: &BuildNode<'src>) -> EmitResult<()> {
        // Collect effects
        let effects_start = self.ctx.effects.len() as u32;
        for effect in &node.effects {
            let ir_effect = self.convert_effect(effect)?;
            self.ctx.effects.push(ir_effect);
        }
        let effects_len = (self.ctx.effects.len() as u32 - effects_start) as u16;
        self.ctx
            .transition_effects
            .push(Slice::new(effects_start, effects_len));

        // Collect negated fields
        let negated_start = self.ctx.negated_fields.len() as u32;
        if let BuildMatcher::Node { negated_fields, .. } = &node.matcher {
            for field_name in negated_fields {
                let field_id = self
                    .resolver
                    .resolve_field(field_name)
                    .ok_or_else(|| EmitError::UnknownField((*field_name).to_string()))?;
                self.ctx.negated_fields.push(field_id);
            }
        }
        let negated_len = (self.ctx.negated_fields.len() as u32 - negated_start) as u16;
        self.ctx
            .transition_negated_fields
            .push(Slice::new(negated_start, negated_len));

        // Check if successors need to spill
        if node.successors.len() > MAX_INLINE_SUCCESSORS {
            let start = self.ctx.spilled_successors.len() as u32;
            for &succ in &node.successors {
                self.ctx.spilled_successors.push(succ);
            }
            self.ctx
                .transition_spilled
                .push(Some((start, node.successors.len() as u32)));
        } else {
            self.ctx.transition_spilled.push(None);
        }

        Ok(())
    }

    fn convert_effect(&mut self, effect: &BuildEffect<'src>) -> EmitResult<EffectOp> {
        Ok(match effect {
            BuildEffect::CaptureNode => EffectOp::CaptureNode,
            BuildEffect::ClearCurrent => EffectOp::ClearCurrent,
            BuildEffect::StartArray { .. } => EffectOp::StartArray,
            BuildEffect::PushElement => EffectOp::PushElement,
            BuildEffect::EndArray => EffectOp::EndArray,
            BuildEffect::StartObject { .. } => EffectOp::StartObject,
            BuildEffect::EndObject => EffectOp::EndObject,
            BuildEffect::Field { name, .. } => {
                let id = self.ctx.intern(name);
                EffectOp::Field(id)
            }
            BuildEffect::StartVariant(tag) => {
                let id = self.ctx.intern(tag);
                EffectOp::StartVariant(id)
            }
            BuildEffect::EndVariant => EffectOp::EndVariant,
            BuildEffect::ToString => EffectOp::ToString,
        })
    }

    fn compute_layout(&self) -> EmitResult<LayoutInfo> {
        let transition_count = self.ctx.graph.len() as u32;
        let successor_count = self.ctx.spilled_successors.len() as u32;
        let effect_count = self.ctx.effects.len() as u32;
        let negated_field_count = self.ctx.negated_fields.len() as u16;
        let string_ref_count = self.ctx.strings.len() as u16;
        let type_def_count = self.ctx.type_info.type_defs.len() as u16;
        let type_member_count: u16 = self
            .ctx
            .type_info
            .type_defs
            .iter()
            .map(|td| td.members.len() as u16)
            .sum();
        let entrypoint_count = self.ctx.graph.definitions().count() as u16;
        let trivia_kind_count = self.trivia_kinds.len() as u16;

        // Compute offsets with proper alignment
        let mut offset: u32 = 0;

        // Transitions at offset 0, 64-byte aligned
        offset += transition_count * 64;

        // Successors: align 4
        let successors_offset = align_up(offset, 4);
        offset = successors_offset + successor_count * 4;

        // Effects: align 4 (EffectOp is 4 bytes with repr(C, u16) but discriminant+payload)
        let effects_offset = align_up(offset, 4);
        offset = effects_offset + effect_count * 4;

        // Negated fields: align 2
        let negated_fields_offset = align_up(offset, 2);
        offset = negated_fields_offset + (negated_field_count as u32) * 2;

        // String refs: align 4
        let string_refs_offset = align_up(offset, 4);
        offset = string_refs_offset + (string_ref_count as u32) * 8;

        // String bytes: align 1
        let string_bytes_offset = offset;
        offset += self.ctx.strings.total_bytes() as u32;

        // Type defs: align 4
        let type_defs_offset = align_up(offset, 4);
        offset = type_defs_offset + (type_def_count as u32) * 12;

        // Type members: align 2
        let type_members_offset = align_up(offset, 2);
        offset = type_members_offset + (type_member_count as u32) * 4;

        // Entrypoints: align 4
        let entrypoints_offset = align_up(offset, 4);
        offset = entrypoints_offset + (entrypoint_count as u32) * 12;

        // Trivia kinds: align 2
        let trivia_kinds_offset = if trivia_kind_count > 0 {
            let aligned = align_up(offset, 2);
            offset = aligned + (trivia_kind_count as u32) * 2;
            aligned
        } else {
            0
        };

        // Final buffer size, aligned to 64 for potential mmap
        let buffer_len = align_up(offset, 64) as usize;

        Ok(LayoutInfo {
            buffer_len,
            successors_offset,
            effects_offset,
            negated_fields_offset,
            string_refs_offset,
            string_bytes_offset,
            type_defs_offset,
            type_members_offset,
            entrypoints_offset,
            trivia_kinds_offset,
            transition_count,
            successor_count,
            effect_count,
            negated_field_count,
            string_ref_count,
            type_def_count,
            type_member_count,
            entrypoint_count,
            trivia_kind_count,
        })
    }

    fn emit_buffer(self, layout: LayoutInfo) -> EmitResult<CompiledQuery> {
        let mut buffer = CompiledQueryBuffer::allocate(layout.buffer_len);
        let base = buffer.as_mut_ptr();

        // Emit transitions
        self.emit_transitions(base, &layout)?;

        // Emit successors
        self.emit_successors(base, &layout);

        // Emit effects
        self.emit_effects(base, &layout);

        // Emit negated fields
        self.emit_negated_fields(base, &layout);

        // Emit strings
        self.emit_strings(base, &layout);

        // Emit type metadata
        self.emit_types(base, &layout);

        // Emit entrypoints
        self.emit_entrypoints(base, &layout)?;

        // Emit trivia kinds
        self.emit_trivia_kinds(base, &layout);

        Ok(CompiledQuery::new(
            buffer,
            layout.successors_offset,
            layout.effects_offset,
            layout.negated_fields_offset,
            layout.string_refs_offset,
            layout.string_bytes_offset,
            layout.type_defs_offset,
            layout.type_members_offset,
            layout.entrypoints_offset,
            layout.trivia_kinds_offset,
            layout.transition_count,
            layout.successor_count,
            layout.effect_count,
            layout.negated_field_count,
            layout.string_ref_count,
            layout.type_def_count,
            layout.type_member_count,
            layout.entrypoint_count,
            layout.trivia_kind_count,
        ))
    }

    fn emit_transitions(&self, base: *mut u8, _layout: &LayoutInfo) -> EmitResult<()> {
        let transitions_ptr = base as *mut Transition;

        for (idx, (_, node)) in self.ctx.graph.iter().enumerate() {
            let transition = self.build_transition(node, idx)?;
            // SAFETY: buffer is properly sized and aligned
            unsafe {
                ptr::write(transitions_ptr.add(idx), transition);
            }
        }

        Ok(())
    }

    fn build_transition(&self, node: &BuildNode<'src>, idx: usize) -> EmitResult<Transition> {
        let matcher = self.convert_matcher(&node.matcher)?;
        let ref_marker = self.convert_ref_marker(&node.ref_marker);
        let effects = self.ctx.transition_effects[idx];
        let negated_fields_slice = self.ctx.transition_negated_fields[idx];

        // Build successor data
        let (successor_count, successor_data) =
            if let Some((start, count)) = self.ctx.transition_spilled[idx] {
                // Spilled: store index in successor_data[0]
                let mut data = [0u32; MAX_INLINE_SUCCESSORS];
                data[0] = start;
                (count, data)
            } else {
                // Inline
                let mut data = [0u32; MAX_INLINE_SUCCESSORS];
                for (i, &succ) in node.successors.iter().enumerate() {
                    data[i] = succ;
                }
                (node.successors.len() as u32, data)
            };

        // Inject negated_fields into matcher if applicable
        let matcher = match matcher {
            Matcher::Node { kind, field, .. } => Matcher::Node {
                kind,
                field,
                negated_fields: negated_fields_slice,
            },
            Matcher::Anonymous { kind, field, .. } => Matcher::Anonymous {
                kind,
                field,
                negated_fields: Slice::empty(),
            },
            other => other,
        };

        let transition = Transition::new(
            matcher,
            ref_marker,
            node.nav,
            effects,
            successor_count,
            successor_data,
        );

        Ok(transition)
    }

    fn convert_matcher(&self, matcher: &BuildMatcher<'src>) -> EmitResult<Matcher> {
        Ok(match matcher {
            BuildMatcher::Epsilon => Matcher::Epsilon,
            BuildMatcher::Node { kind, field, .. } => {
                let kind_id = self
                    .resolver
                    .resolve_kind(kind)
                    .ok_or_else(|| EmitError::UnknownNodeKind((*kind).to_string()))?;
                let field_id = match field {
                    Some(f) => self.resolver.resolve_field(f),
                    None => None,
                };
                Matcher::Node {
                    kind: kind_id,
                    field: field_id,
                    negated_fields: Slice::empty(), // Will be filled in build_transition
                }
            }
            BuildMatcher::Anonymous { literal, field } => {
                // For anonymous nodes, we use the literal as a synthetic kind ID
                // In practice, this would be resolved differently
                let kind_id = self.resolver.resolve_kind(literal).unwrap_or(0);
                let field_id = match field {
                    Some(f) => self.resolver.resolve_field(f),
                    None => None,
                };
                Matcher::Anonymous {
                    kind: kind_id,
                    field: field_id,
                    negated_fields: Slice::empty(),
                }
            }
            BuildMatcher::Wildcard { field } => {
                // Wildcard doesn't use field in IR representation
                let _ = field;
                Matcher::Wildcard
            }
        })
    }

    fn convert_ref_marker(&self, marker: &RefMarker) -> RefTransition {
        match marker {
            RefMarker::None => RefTransition::None,
            RefMarker::Enter { ref_id } => RefTransition::Enter(*ref_id as RefId),
            RefMarker::Exit { ref_id } => RefTransition::Exit(*ref_id as RefId),
        }
    }

    fn emit_successors(&self, base: *mut u8, layout: &LayoutInfo) {
        if self.ctx.spilled_successors.is_empty() {
            return;
        }

        let ptr = unsafe { base.add(layout.successors_offset as usize) } as *mut TransitionId;
        for (i, &succ) in self.ctx.spilled_successors.iter().enumerate() {
            unsafe {
                ptr::write(ptr.add(i), succ);
            }
        }
    }

    fn emit_effects(&self, base: *mut u8, layout: &LayoutInfo) {
        if self.ctx.effects.is_empty() {
            return;
        }

        let ptr = unsafe { base.add(layout.effects_offset as usize) } as *mut EffectOp;
        for (i, effect) in self.ctx.effects.iter().enumerate() {
            unsafe {
                ptr::write(ptr.add(i), *effect);
            }
        }
    }

    fn emit_negated_fields(&self, base: *mut u8, layout: &LayoutInfo) {
        if self.ctx.negated_fields.is_empty() {
            return;
        }

        let ptr = unsafe { base.add(layout.negated_fields_offset as usize) } as *mut NodeFieldId;
        for (i, &field) in self.ctx.negated_fields.iter().enumerate() {
            unsafe {
                ptr::write(ptr.add(i), field);
            }
        }
    }

    fn emit_strings(&self, base: *mut u8, layout: &LayoutInfo) {
        // Emit string refs
        let refs_ptr = unsafe { base.add(layout.string_refs_offset as usize) } as *mut StringRef;
        let bytes_ptr = unsafe { base.add(layout.string_bytes_offset as usize) };

        let mut byte_offset: u32 = 0;
        for (i, (_, s)) in self.ctx.strings.iter().enumerate() {
            // Write StringRef
            let string_ref = StringRef::new(byte_offset, s.len() as u16);
            unsafe {
                ptr::write(refs_ptr.add(i), string_ref);
            }

            // Write string bytes
            unsafe {
                ptr::copy_nonoverlapping(s.as_ptr(), bytes_ptr.add(byte_offset as usize), s.len());
            }

            byte_offset += s.len() as u32;
        }
    }

    fn emit_types(&self, base: *mut u8, layout: &LayoutInfo) {
        let defs_ptr = unsafe { base.add(layout.type_defs_offset as usize) } as *mut TypeDef;
        let members_ptr =
            unsafe { base.add(layout.type_members_offset as usize) } as *mut TypeMember;

        let mut member_idx: u32 = 0;

        for (i, type_def) in self.ctx.type_info.type_defs.iter().enumerate() {
            let name_id = type_def
                .name
                .and_then(|n| self.ctx.strings.get(n))
                .unwrap_or(super::ids::STRING_NONE);

            let ir_def = if let Some(inner) = type_def.inner_type {
                TypeDef::wrapper(type_def.kind, inner)
            } else {
                let members_start = member_idx;
                let members_len = type_def.members.len() as u16;

                // Emit members
                for member in &type_def.members {
                    let member_name_id = self
                        .ctx
                        .strings
                        .get(member.name)
                        .expect("member name should be interned");
                    let ir_member = TypeMember::new(member_name_id, member.ty);
                    unsafe {
                        ptr::write(members_ptr.add(member_idx as usize), ir_member);
                    }
                    member_idx += 1;
                }

                TypeDef::composite(
                    type_def.kind,
                    name_id,
                    Slice::new(members_start, members_len),
                )
            };

            unsafe {
                ptr::write(defs_ptr.add(i), ir_def);
            }
        }
    }

    fn emit_entrypoints(&self, base: *mut u8, layout: &LayoutInfo) -> EmitResult<()> {
        let ptr = unsafe { base.add(layout.entrypoints_offset as usize) } as *mut Entrypoint;

        for (i, (name, entry_node)) in self.ctx.graph.definitions().enumerate() {
            let name_id = self
                .ctx
                .strings
                .get(name)
                .expect("definition name should be interned");

            // Look up the result type for this definition
            let result_type = self
                .ctx
                .type_info
                .entrypoint_types
                .get(name)
                .copied()
                .unwrap_or(TYPE_NODE);

            let entrypoint = Entrypoint::new(name_id, entry_node, result_type);
            unsafe {
                ptr::write(ptr.add(i), entrypoint);
            }
        }

        Ok(())
    }

    fn emit_trivia_kinds(&self, base: *mut u8, layout: &LayoutInfo) {
        if self.trivia_kinds.is_empty() {
            return;
        }

        let ptr = unsafe { base.add(layout.trivia_kinds_offset as usize) } as *mut NodeTypeId;
        for (i, &kind) in self.trivia_kinds.iter().enumerate() {
            unsafe {
                ptr::write(ptr.add(i), kind);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::graph::{BuildEffect, BuildGraph, BuildMatcher, BuildNode};
    use crate::query::infer::TypeInferenceResult;
    use std::num::NonZeroU16;

    fn make_resolver() -> MapResolver {
        let mut r = MapResolver::new();
        r.add_kind("identifier", 1);
        r.add_kind("function_declaration", 2);
        r.add_field("name", NonZeroU16::new(1).unwrap());
        r.add_field("body", NonZeroU16::new(2).unwrap());
        r
    }

    #[test]
    fn emit_simple_query() {
        let mut graph = BuildGraph::new();

        // Create a simple: (identifier) @id
        let node = graph.add_node(BuildNode::with_matcher(BuildMatcher::node("identifier")));
        graph.node_mut(node).add_effect(BuildEffect::CaptureNode);
        graph.add_definition("Main", node);

        let type_info = TypeInferenceResult::default();
        let resolver = make_resolver();

        let emitter = QueryEmitter::new(&graph, &type_info, resolver);
        let compiled = emitter.emit().expect("emit should succeed");

        assert_eq!(compiled.transition_count(), 1);
        assert_eq!(compiled.entrypoint_count(), 1);

        let t = compiled.transition(0);
        assert!(matches!(t.matcher, Matcher::Node { kind: 1, .. }));
    }

    #[test]
    fn emit_with_effects() {
        let mut graph = BuildGraph::new();

        let node = graph.add_node(BuildNode::with_matcher(BuildMatcher::node("identifier")));
        graph.node_mut(node).add_effect(BuildEffect::CaptureNode);
        graph.node_mut(node).add_effect(BuildEffect::Field {
            name: "name",
            span: Default::default(),
        });
        graph.add_definition("Main", node);

        let type_info = TypeInferenceResult::default();
        let resolver = make_resolver();

        let emitter = QueryEmitter::new(&graph, &type_info, resolver);
        let compiled = emitter.emit().expect("emit should succeed");

        let view = compiled.transition_view(0);
        let effects = view.effects();
        assert_eq!(effects.len(), 2);
        assert!(matches!(effects[0], EffectOp::CaptureNode));
        assert!(matches!(effects[1], EffectOp::Field(_)));

        // Verify string was interned
        if let EffectOp::Field(id) = effects[1] {
            assert_eq!(compiled.string(id), "name");
        }
    }

    #[test]
    fn emit_with_successors() {
        let mut graph = BuildGraph::new();

        // Create: entry -> branch -> [a, b]
        let entry = graph.add_epsilon();
        let a = graph.add_node(BuildNode::with_matcher(BuildMatcher::node("identifier")));
        let b = graph.add_node(BuildNode::with_matcher(BuildMatcher::node(
            "function_declaration",
        )));
        graph.connect(entry, a);
        graph.connect(entry, b);
        graph.add_definition("Main", entry);

        let type_info = TypeInferenceResult::default();
        let resolver = make_resolver();

        let emitter = QueryEmitter::new(&graph, &type_info, resolver);
        let compiled = emitter.emit().expect("emit should succeed");

        assert_eq!(compiled.transition_count(), 3);

        let view = compiled.transition_view(0);
        let successors = view.successors();
        assert_eq!(successors.len(), 2);
        assert_eq!(successors[0], 1);
        assert_eq!(successors[1], 2);
    }

    #[test]
    fn emit_many_successors_spills() {
        let mut graph = BuildGraph::new();

        // Create entry with 10 successors (exceeds MAX_INLINE_SUCCESSORS)
        let entry = graph.add_epsilon();
        for _ in 0..10 {
            let node = graph.add_node(BuildNode::with_matcher(BuildMatcher::node("identifier")));
            graph.connect(entry, node);
        }
        graph.add_definition("Main", entry);

        let type_info = TypeInferenceResult::default();
        let resolver = make_resolver();

        let emitter = QueryEmitter::new(&graph, &type_info, resolver);
        let compiled = emitter.emit().expect("emit should succeed");

        let t = compiled.transition(0);
        assert!(!t.has_inline_successors());
        assert_eq!(t.successor_count, 10);

        let view = compiled.transition_view(0);
        let successors = view.successors();
        assert_eq!(successors.len(), 10);
    }

    #[test]
    fn string_interning_deduplicates() {
        let mut graph = BuildGraph::new();

        // Two fields with same name
        let n1 = graph.add_node(BuildNode::with_matcher(BuildMatcher::node("identifier")));
        graph.node_mut(n1).add_effect(BuildEffect::Field {
            name: "value",
            span: Default::default(),
        });

        let n2 = graph.add_node(BuildNode::with_matcher(BuildMatcher::node("identifier")));
        graph.node_mut(n2).add_effect(BuildEffect::Field {
            name: "value",
            span: Default::default(),
        });
        graph.connect(n1, n2);

        graph.add_definition("Main", n1);

        let type_info = TypeInferenceResult::default();
        let resolver = make_resolver();

        let emitter = QueryEmitter::new(&graph, &type_info, resolver);
        let compiled = emitter.emit().expect("emit should succeed");

        // Both should reference the same string ID
        let e1 = compiled.transition_view(0).effects();
        let e2 = compiled.transition_view(1).effects();

        let id1 = match e1[0] {
            EffectOp::Field(id) => id,
            _ => panic!(),
        };
        let id2 = match e2[0] {
            EffectOp::Field(id) => id,
            _ => panic!(),
        };

        assert_eq!(id1, id2);
        assert_eq!(compiled.string(id1), "value");
    }

    #[test]
    fn unknown_node_kind_errors() {
        let mut graph = BuildGraph::new();
        let node = graph.add_node(BuildNode::with_matcher(BuildMatcher::node("unknown_kind")));
        graph.add_definition("Main", node);

        let type_info = TypeInferenceResult::default();
        let resolver = make_resolver();

        let emitter = QueryEmitter::new(&graph, &type_info, resolver);
        let result = emitter.emit();

        assert!(matches!(result, Err(EmitError::UnknownNodeKind(_))));
    }

    #[test]
    fn serialize_deserialize_roundtrip() {
        let mut graph = BuildGraph::new();

        // Build a small graph with effects
        let n1 = graph.add_node(BuildNode::with_matcher(BuildMatcher::node("identifier")));
        graph.node_mut(n1).add_effect(BuildEffect::CaptureNode);
        graph.node_mut(n1).add_effect(BuildEffect::Field {
            name: "id",
            span: Default::default(),
        });

        let n2 = graph.add_node(BuildNode::with_matcher(BuildMatcher::node(
            "function_declaration",
        )));
        graph.node_mut(n2).add_effect(BuildEffect::CaptureNode);
        graph.connect(n1, n2);

        graph.add_definition("Main", n1);

        let type_info = TypeInferenceResult::default();
        let resolver = make_resolver();

        // Emit
        let emitter = QueryEmitter::new(&graph, &type_info, resolver);
        let compiled = emitter.emit().expect("emit should succeed");

        // Serialize
        let bytes = crate::ir::to_bytes(&compiled).expect("serialize should succeed");

        // Deserialize
        let restored = crate::ir::from_bytes(&bytes).expect("deserialize should succeed");

        // Verify counts
        assert_eq!(restored.transition_count(), compiled.transition_count());
        assert_eq!(restored.entrypoint_count(), compiled.entrypoint_count());

        // Check transitions match
        for i in 0..compiled.transition_count() {
            let orig = compiled.transition_view(i);
            let rest = restored.transition_view(i);

            assert_eq!(orig.successors(), rest.successors());
            assert_eq!(orig.effects().len(), rest.effects().len());
        }

        // Check strings match
        let ep = restored.entrypoints()[0];
        assert_eq!(restored.string(ep.name_id()), "Main");
    }

    #[test]
    fn dump_produces_output() {
        let mut graph = BuildGraph::new();
        let node = graph.add_node(BuildNode::with_matcher(BuildMatcher::node("identifier")));
        graph.node_mut(node).add_effect(BuildEffect::CaptureNode);
        graph.add_definition("Test", node);

        let type_info = TypeInferenceResult::default();
        let resolver = make_resolver();

        let emitter = QueryEmitter::new(&graph, &type_info, resolver);
        let compiled = emitter.emit().expect("emit should succeed");

        let dump = compiled.dump();

        assert!(dump.contains("CompiledQuery"));
        assert!(dump.contains("Test"));
        assert!(dump.contains("Capture"));
        assert!(dump.contains("Node(1)"));
    }
}
