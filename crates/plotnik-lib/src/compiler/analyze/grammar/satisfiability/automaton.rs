//! Child automata compiled from resolved matching structure. Each caller gets its
//! own sequence exits, while nested node obligations share their pattern IDs.

use std::collections::HashMap;

use crate::compiler::analyze::anchors::GapClass;
use crate::compiler::analyze::boundary::{BoundaryState, FirstClass, PendingAnchor};
use crate::compiler::diagnostics::Span;
use crate::compiler::ids::DefId;
use crate::compiler::parse::ast::QuantifierKind;
use crate::core::NodeFieldId;

use super::super::model::{Item, KindConstraint, Model, PatternId, PatternKind, Repeat};
use super::AnalysisLimit;

pub(super) type State = u32;

#[derive(Clone, Copy, Debug)]
pub(super) struct ChildMatcher {
    pub(super) kind: KindConstraint,
    pub(super) nested_pattern: Option<PatternId>,
    pub(super) field: Option<NodeFieldId>,
}

impl ChildMatcher {
    pub(super) fn new(kind: KindConstraint, field: Option<NodeFieldId>) -> Self {
        Self {
            kind,
            field,
            nested_pattern: None,
        }
    }

    fn node(
        node: &super::super::model::Node,
        pattern: PatternId,
        field: Option<NodeFieldId>,
    ) -> Self {
        Self {
            nested_pattern: node.constrains_children().then_some(pattern),
            ..Self::new(node.kind, field)
        }
    }
}

#[derive(Clone, Debug)]
struct StateData {
    anchor: Option<(PendingAnchor, Span)>,
    entry_states: u16,
    entry_gap: GapClass,
    pattern_edges: Vec<(ChildMatcher, State)>,
    eps_edges: Vec<State>,
}

impl StateData {
    fn new() -> Self {
        Self {
            anchor: None,
            entry_states: (1 << BoundaryState::COUNT) - 1,
            entry_gap: GapClass::Any,
            pattern_edges: Vec::new(),
            eps_edges: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct ChildAutomaton {
    states: Vec<StateData>,
    start: State,
    accept: State,
    negated_fields: Vec<NodeFieldId>,
}

impl ChildAutomaton {
    pub(super) fn remove_anchor(&mut self, span: Span) {
        for state in &mut self.states {
            if state.anchor.is_some_and(|(_, source)| source == span) {
                state.anchor = None;
            }
        }
    }

    pub(super) fn anchor(&self, state: State) -> Option<(PendingAnchor, Span)> {
        self.states[state as usize].anchor
    }

    pub(super) fn advance(&self, state: State, boundary: BoundaryState) -> Option<BoundaryState> {
        let data = &self.states[state as usize];
        if data.entry_states & (1 << boundary.index()) == 0 {
            return None;
        }
        Some(
            data.anchor
                .map_or(boundary, |(anchor, _)| boundary.tighten(anchor)),
        )
    }

    pub(super) fn gap(&self, state: State, boundary: BoundaryState, next: FirstClass) -> GapClass {
        boundary
            .gap_before(next)
            .tighten(self.states[state as usize].entry_gap)
    }

    pub(super) fn allow_field(&mut self, field: NodeFieldId) {
        self.negated_fields.retain(|&id| id != field);
    }

    pub(super) fn without_field(field: NodeFieldId) -> Self {
        Self {
            states: vec![StateData::new()],
            start: 0,
            accept: 0,
            negated_fields: vec![field],
        }
    }

    pub(super) fn allow_nested(&mut self, pattern: PatternId) {
        for state in &mut self.states {
            for (matcher, _) in &mut state.pattern_edges {
                if matcher.nested_pattern == Some(pattern) {
                    matcher.nested_pattern = None;
                }
            }
        }
    }

    /// An order probe asks about two resolved demands without carrying the rest
    /// of the failed query's restrictions into the answer.
    pub(super) fn ordered_pair(first: ChildMatcher, second: ChildMatcher) -> Self {
        let mut states: Vec<_> = (0..3).map(|_| StateData::new()).collect();
        states[0].pattern_edges.push((first, 1));
        states[1].pattern_edges.push((second, 2));
        Self {
            states,
            start: 0,
            accept: 2,
            negated_fields: Vec::new(),
        }
    }

    pub(super) fn start(&self) -> State {
        self.start
    }

    pub(super) fn accept(&self) -> State {
        self.accept
    }

    pub(super) fn negates(&self, field: Option<NodeFieldId>) -> bool {
        field.is_some_and(|field| self.negated_fields.contains(&field))
    }

    pub(super) fn state_count(&self) -> usize {
        self.states.len()
    }

    pub(super) fn pattern_edges(&self, state: State) -> &[(ChildMatcher, State)] {
        &self.states[state as usize].pattern_edges
    }

    pub(super) fn eps_edges(&self, state: State) -> &[State] {
        &self.states[state as usize].eps_edges
    }

    pub(super) fn nested_patterns(&self) -> impl Iterator<Item = PatternId> + '_ {
        self.states
            .iter()
            .flat_map(|state| &state.pattern_edges)
            .filter_map(|(matcher, _)| matcher.nested_pattern)
    }
}

pub(super) fn build(
    model: &Model,
    pattern: PatternId,
    max_depth: u32,
    state_budget: u64,
) -> Result<ChildAutomaton, AnalysisLimit> {
    let node = model.node(pattern);
    let mut builder = Builder {
        model,
        states: Vec::new(),
        state_budget,
        boundary_inputs: model.boundary_inputs().collect(),
        ref_stack: Vec::new(),
        max_depth,
        depth: 0,
    };
    let start = builder.new_state()?;
    let accept = builder.emit_items(&node.children, start)?;
    Ok(ChildAutomaton {
        states: builder.states,
        start,
        accept,
        negated_fields: node.negated_fields.iter().map(|field| field.id).collect(),
    })
}

struct Builder<'a> {
    model: &'a Model,
    states: Vec<StateData>,
    state_budget: u64,
    boundary_inputs: Vec<BoundaryState>,
    ref_stack: Vec<DefId>,
    max_depth: u32,
    depth: u32,
}

// Reference diamonds and nested repeats can expand faster than their source.
const STATE_CAP: usize = 20_000;

impl Builder<'_> {
    fn new_state(&mut self) -> Result<State, AnalysisLimit> {
        if self.states.len() >= STATE_CAP || self.states.len() as u64 >= self.state_budget {
            return Err(AnalysisLimit);
        }

        let id = self.states.len() as State;
        self.states.push(StateData::new());
        Ok(id)
    }

    fn emit_items(&mut self, items: &[Item], entry: State) -> Result<State, AnalysisLimit> {
        let mut current = entry;
        for item in items {
            match item {
                Item::Pattern(pattern) => {
                    // A sibling list does not inherit a field from its surrounding group.
                    current = self.emit_pattern(*pattern, None, current)?;
                }
                Item::Anchor(anchor, span) => {
                    self.states[current as usize].anchor = Some((*anchor, *span));
                    let next = self.new_state()?;
                    self.states[current as usize].eps_edges.push(next);
                    current = next;
                }
            }
        }
        Ok(current)
    }

    fn emit_pattern(
        &mut self,
        pattern: PatternId,
        field: Option<NodeFieldId>,
        from: State,
    ) -> Result<State, AnalysisLimit> {
        if self.depth >= self.max_depth {
            return Err(AnalysisLimit);
        }

        self.depth += 1;
        let exit = self.emit_inner(pattern, field, from);
        self.depth -= 1;
        exit
    }

    fn emit_inner(
        &mut self,
        pattern: PatternId,
        field: Option<NodeFieldId>,
        from: State,
    ) -> Result<State, AnalysisLimit> {
        match &self.model.pattern(pattern).kind {
            PatternKind::Node(node) if node.violation.is_some() => self.new_state(),
            PatternKind::Node(node) => {
                self.emit_single(ChildMatcher::node(node, pattern, field), from)
            }
            PatternKind::Leaf(kind) => self.emit_single(ChildMatcher::new(*kind, field), from),
            PatternKind::Field(field, inner) => self.emit_pattern(*inner, Some(*field), from),
            PatternKind::Sequence(items) => self.emit_items(items, from),
            PatternKind::Repeat(repeat) => self.emit_repeat(repeat, field, from),
            PatternKind::Alternative(alternatives) => {
                let to = self.new_state()?;
                for &alternative in alternatives {
                    let branch = self.new_state()?;
                    self.states[from as usize].eps_edges.push(branch);
                    let exit = self.emit_pattern(alternative, field, branch)?;
                    self.states[exit as usize].eps_edges.push(to);
                }
                if alternatives.is_empty() {
                    self.states[from as usize].eps_edges.push(to);
                }
                Ok(to)
            }
            PatternKind::Reference(definition) => self.emit_reference(*definition, field, from),
        }
    }

    fn emit_single(&mut self, matcher: ChildMatcher, from: State) -> Result<State, AnalysisLimit> {
        let to = self.new_state()?;
        self.states[from as usize].pattern_edges.push((matcher, to));
        Ok(to)
    }

    fn emit_repeat(
        &mut self,
        repeat: &Repeat,
        field: Option<NodeFieldId>,
        from: State,
    ) -> Result<State, AnalysisLimit> {
        let to = self.new_state()?;
        if repeat.kind != QuantifierKind::OneOrMore {
            self.states[from as usize].eps_edges.push(to);
        }
        // A simple repeat reuses its entry search permission. Keep that finite
        // choice in loop control, while BoundaryState retains path-specific
        // previous consumers and pending anchors throughout the body.
        let mut entries = Vec::new();
        for &input in &self.boundary_inputs {
            let gap = if repeat.kind == QuantifierKind::Optional {
                None
            } else {
                repeat.entry_gaps[input.index()]
            };
            if let Some((_, inputs)) = entries.iter_mut().find(|(seen, _)| *seen == gap) {
                *inputs |= 1u16 << input.index();
            } else {
                entries.push((gap, 1u16 << input.index()));
            }
        }
        for (gap, inputs) in entries {
            let branch = self.new_state()?;
            let body = self.new_state()?;
            let exit = self.emit_pattern(repeat.inner, field, body)?;
            let consuming = self.consuming_entry(body, exit, GapClass::Any)?;
            self.states[from as usize].eps_edges.push(branch);
            self.states[branch as usize].entry_states = inputs;
            self.states[branch as usize].eps_edges.push(consuming);
            self.states[exit as usize].eps_edges.push(to);
            if repeat.kind != QuantifierKind::Optional {
                let loop_entry = if let Some(gap) = gap {
                    self.consuming_entry(body, exit, gap)?
                } else {
                    consuming
                };
                self.states[exit as usize].eps_edges.push(loop_entry);
            }
        }
        Ok(to)
    }

    /// An iteration must consume. Copy only its epsilon prefix, with no route
    /// to its empty exit. The first consuming edge joins the original body.
    /// Loop entries also retain the search permission resolved at repeat entry.
    fn consuming_entry(
        &mut self,
        entry: State,
        exit: State,
        entry_gap: GapClass,
    ) -> Result<State, AnalysisLimit> {
        let start = self.new_state()?;
        let mut copies = HashMap::from([(entry, start)]);
        let mut pending = vec![entry];
        while let Some(state) = pending.pop() {
            let copy = copies[&state];
            if state == exit {
                continue;
            }
            let original = self.states[state as usize].clone();
            self.states[copy as usize].anchor = original.anchor;
            self.states[copy as usize].entry_states = original.entry_states;
            self.states[copy as usize].entry_gap = original.entry_gap.tighten(entry_gap);
            self.states[copy as usize].pattern_edges = original.pattern_edges;
            for next in original.eps_edges {
                let target = if let Some(&target) = copies.get(&next) {
                    target
                } else {
                    let target = self.new_state()?;
                    copies.insert(next, target);
                    pending.push(next);
                    target
                };
                self.states[copy as usize].eps_edges.push(target);
            }
        }
        Ok(start)
    }

    fn emit_reference(
        &mut self,
        definition: DefId,
        field: Option<NodeFieldId>,
        from: State,
    ) -> Result<State, AnalysisLimit> {
        // Sibling recursion is not generally regular. Keep the existing conservative
        // tail loop while retaining constraints imposed by the non-recursive prefix.
        if self.ref_stack.contains(&definition) {
            let exit = self.new_state()?;
            let consumer = self.new_state()?;
            self.states[from as usize]
                .eps_edges
                .extend([consumer, exit]);
            self.states[consumer as usize]
                .pattern_edges
                .push((ChildMatcher::new(KindConstraint::AnyNode, None), from));
            return Ok(exit);
        }

        let target = self.model.definition(definition);
        if matches!(self.model.pattern(target).kind, PatternKind::Node(_)) {
            return self.emit_inner(target, field, from);
        }

        self.ref_stack.push(definition);
        let exit = self.emit_pattern(target, field, from);
        self.ref_stack.pop();
        exit
    }
}
