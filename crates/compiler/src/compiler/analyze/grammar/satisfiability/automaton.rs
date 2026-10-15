use std::collections::HashMap;

use crate::compiler::analyze::anchors::GapClass;
use crate::compiler::analyze::boundary::{BoundaryState, FirstClass, PendingAnchor};
use crate::compiler::diagnostics::Span;
use crate::compiler::ids::DefId;
use crate::compiler::parse::ast::QuantifierKind;
use crate::core::NodeFieldId;

use super::super::model::{Item, KindConstraint, Model, PatternId, PatternKind, Repeat};
use super::AnalysisLimit;
use super::budget::Budget;

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
    pub(super) fn remove_anchor(
        &mut self,
        span: Span,
        budget: &mut Budget,
    ) -> Result<(), AnalysisLimit> {
        budget.spend(self.states.len() as u64)?;
        for state in &mut self.states {
            if state.anchor.is_some_and(|(_, source)| source == span) {
                state.anchor = None;
            }
        }
        Ok(())
    }

    pub(super) fn clear_anchors(&mut self, budget: &mut Budget) -> Result<(), AnalysisLimit> {
        budget.spend(self.states.len() as u64)?;
        for state in &mut self.states {
            state.anchor = None;
        }
        Ok(())
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

    pub(super) fn allow_field(
        &mut self,
        field: NodeFieldId,
        budget: &mut Budget,
    ) -> Result<(), AnalysisLimit> {
        budget.spend(self.negated_fields.len() as u64)?;
        self.negated_fields.retain(|&id| id != field);
        Ok(())
    }

    pub(super) fn without_field(
        field: NodeFieldId,
        budget: &mut Budget,
    ) -> Result<Self, AnalysisLimit> {
        let mut automaton = Self::demands(&[], budget)?;
        budget.charge()?;
        automaton.negated_fields.push(field);
        Ok(automaton)
    }

    pub(super) fn allow_nested(
        &mut self,
        pattern: PatternId,
        budget: &mut Budget,
    ) -> Result<(), AnalysisLimit> {
        budget.spend(self.states.len() as u64)?;
        for state in &mut self.states {
            budget.spend(state.pattern_edges.len() as u64)?;
            for (matcher, _) in &mut state.pattern_edges {
                if matcher.nested_pattern == Some(pattern) {
                    matcher.nested_pattern = None;
                }
            }
        }
        Ok(())
    }

    pub(super) fn ordered_pair(
        first: ChildMatcher,
        second: ChildMatcher,
        budget: &mut Budget,
    ) -> Result<Self, AnalysisLimit> {
        Self::demands(&[first, second], budget)
    }

    pub(super) fn child(matcher: ChildMatcher, budget: &mut Budget) -> Result<Self, AnalysisLimit> {
        Self::demands(&[matcher], budget)
    }

    fn demands(matchers: &[ChildMatcher], budget: &mut Budget) -> Result<Self, AnalysisLimit> {
        budget.spend((matchers.len() * 2 + 1) as u64)?;
        let mut states: Vec<_> = (0..=matchers.len()).map(|_| StateData::new()).collect();
        for (index, &matcher) in matchers.iter().enumerate() {
            states[index]
                .pattern_edges
                .push((matcher, index as State + 1));
        }
        Ok(Self {
            states,
            start: 0,
            accept: matchers.len() as State,
            negated_fields: Vec::new(),
        })
    }

    pub(super) fn copy(&self, budget: &mut Budget) -> Result<Self, AnalysisLimit> {
        budget.spend(self.states.len() as u64)?;
        budget.spend(self.work_size())?;
        Ok(self.clone())
    }

    pub(super) fn work_size(&self) -> u64 {
        (self.states.len()
            + self.negated_fields.len()
            + self
                .states
                .iter()
                .map(|state| state.pattern_edges.len() + state.eps_edges.len())
                .sum::<usize>()) as u64
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
    budget: &mut Budget,
) -> Result<ChildAutomaton, AnalysisLimit> {
    budget.spend(BoundaryState::COUNT as u64)?;
    let node = model.node(pattern);
    let mut builder = Builder {
        model,
        states: Vec::new(),
        budget,
        boundary_inputs: model.boundary_inputs().collect(),
        ref_stack: Vec::new(),
        max_depth,
        depth: 0,
    };
    let start = builder.new_state()?;
    let accept = builder.emit_items(&node.children, start)?;
    builder.budget.spend(node.negated_fields.len() as u64)?;
    Ok(ChildAutomaton {
        states: builder.states,
        start,
        accept,
        negated_fields: node.negated_fields.iter().map(|field| field.id).collect(),
    })
}

struct Builder<'q, 'a> {
    model: &'q Model,
    states: Vec<StateData>,
    budget: &'a mut Budget,
    boundary_inputs: Vec<BoundaryState>,
    ref_stack: Vec<DefId>,
    max_depth: u32,
    depth: u32,
}

const STATE_CAP: usize = 20_000;

impl Builder<'_, '_> {
    fn new_state(&mut self) -> Result<State, AnalysisLimit> {
        if self.states.len() >= STATE_CAP {
            return Err(AnalysisLimit::AutomatonStates);
        }

        self.budget.charge()?;
        let id = self.states.len() as State;
        self.states.push(StateData::new());
        Ok(id)
    }

    fn connect(&mut self, from: State, to: State) -> Result<(), AnalysisLimit> {
        self.budget.charge()?;
        self.states[from as usize].eps_edges.push(to);
        Ok(())
    }

    fn emit_items(&mut self, items: &[Item], entry: State) -> Result<State, AnalysisLimit> {
        let mut current = entry;
        for item in items {
            self.budget.charge()?;
            match item {
                Item::Pattern(pattern) => {
                    current = self.emit_pattern(*pattern, None, current)?;
                }
                Item::Anchor(anchor, span) => {
                    self.states[current as usize].anchor = Some((*anchor, *span));
                    let next = self.new_state()?;
                    self.connect(current, next)?;
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
            return Err(AnalysisLimit::ConstructionDepth);
        }

        self.budget.charge()?;
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
                    self.connect(from, branch)?;
                    let exit = self.emit_pattern(alternative, field, branch)?;
                    self.connect(exit, to)?;
                }
                if alternatives.is_empty() {
                    self.connect(from, to)?;
                }
                Ok(to)
            }
            PatternKind::Reference(definition) => self.emit_reference(*definition, field, from),
        }
    }

    fn emit_single(&mut self, matcher: ChildMatcher, from: State) -> Result<State, AnalysisLimit> {
        let to = self.new_state()?;
        self.budget.charge()?;
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
            self.connect(from, to)?;
        }
        let mut entries = Vec::new();
        for &input in &self.boundary_inputs {
            self.budget.spend(entries.len() as u64 + 1)?;
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
            self.connect(from, branch)?;
            self.states[branch as usize].entry_states = inputs;
            self.connect(branch, consuming)?;
            self.connect(exit, to)?;
            if repeat.kind != QuantifierKind::Optional {
                let loop_entry = if let Some(gap) = gap {
                    self.consuming_entry(body, exit, gap)?
                } else {
                    consuming
                };
                self.connect(exit, loop_entry)?;
            }
        }
        Ok(to)
    }

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
            self.budget.charge()?;
            let copy = copies[&state];
            if state == exit {
                continue;
            }
            let original = &self.states[state as usize];
            self.budget
                .spend((original.pattern_edges.len() + original.eps_edges.len() + 1) as u64)?;
            let original = original.clone();
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
                self.connect(copy, target)?;
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
        self.budget.spend(self.ref_stack.len() as u64 + 1)?;
        if self.ref_stack.contains(&definition) {
            let exit = self.new_state()?;
            let consumer = self.new_state()?;
            self.connect(from, consumer)?;
            self.connect(from, exit)?;
            self.budget.charge()?;
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
