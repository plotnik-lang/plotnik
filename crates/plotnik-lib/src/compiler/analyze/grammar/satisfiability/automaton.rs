//! Child automata compiled from resolved matching structure. Each caller gets its
//! own sequence exits, while nested node obligations share their pattern IDs.

use crate::compiler::analyze::anchors::GapClass;
use crate::compiler::diagnostics::Span;
use crate::compiler::ids::DefId;
use crate::compiler::parse::ast::QuantifierKind;
use crate::core::NodeFieldId;

use super::super::model::{Item, KindConstraint, Model, PatternId, PatternKind};
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
    gap: GapClass,
    anchor: Option<Span>,
    pattern_edges: Vec<(ChildMatcher, State)>,
    eps_edges: Vec<State>,
}

impl StateData {
    fn new(gap: GapClass) -> Self {
        Self {
            gap,
            anchor: None,
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
    pub(super) fn set_gap(&mut self, state: State, gap: GapClass) {
        self.states[state as usize].gap = gap;
    }

    pub(super) fn anchor(&self, state: State) -> Option<Span> {
        self.states[state as usize].anchor
    }

    pub(super) fn allow_field(&mut self, field: NodeFieldId) {
        self.negated_fields.retain(|&id| id != field);
    }

    pub(super) fn without_field(field: NodeFieldId) -> Self {
        Self {
            states: vec![StateData::new(GapClass::Any)],
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
        let mut states: Vec<_> = (0..3).map(|_| StateData::new(GapClass::Any)).collect();
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

    pub(super) fn gap(&self, state: State) -> GapClass {
        self.states[state as usize].gap
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
        ref_stack: Vec::new(),
        max_depth,
        depth: 0,
    };
    let start = builder.new_state(GapClass::Any)?;
    let accept = builder.emit_items(&node.children, start, None)?;
    builder.states[accept as usize].gap = node.trailing;
    builder.states[accept as usize].anchor = node.trailing_anchor;
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
    ref_stack: Vec<DefId>,
    max_depth: u32,
    depth: u32,
}

// Reference diamonds can expand exponentially even when their source is small.
const STATE_CAP: usize = 20_000;

impl Builder<'_> {
    fn new_state(&mut self, gap: GapClass) -> Result<State, AnalysisLimit> {
        if self.states.len() >= STATE_CAP || self.states.len() as u64 >= self.state_budget {
            return Err(AnalysisLimit);
        }

        let id = self.states.len() as State;
        self.states.push(StateData::new(gap));
        Ok(id)
    }

    fn emit_items(
        &mut self,
        items: &[Item],
        entry: State,
        inherited_first_gap: Option<(GapClass, Option<Span>)>,
    ) -> Result<State, AnalysisLimit> {
        let mut current = entry;
        for (index, item) in items.iter().enumerate() {
            let (gap, anchor) = if index == 0 {
                inherited_first_gap.unwrap_or((item.gap, item.anchor))
            } else {
                (item.gap, item.anchor)
            };
            self.states[current as usize].gap = gap;
            self.states[current as usize].anchor = anchor;
            // A sibling list does not inherit a field from its surrounding group.
            current = self.emit_pattern(item.pattern, None, current)?;
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
            PatternKind::Node(node) if node.violation.is_some() => self.new_state(GapClass::Any),
            PatternKind::Node(node) => {
                self.emit_single(ChildMatcher::node(node, pattern, field), from)
            }
            PatternKind::Leaf(kind) => self.emit_single(ChildMatcher::new(*kind, field), from),
            PatternKind::Field(field, inner) => self.emit_pattern(*inner, Some(*field), from),
            PatternKind::Sequence(items) => self.emit_items(
                items,
                from,
                Some((
                    self.states[from as usize].gap,
                    self.states[from as usize].anchor,
                )),
            ),
            PatternKind::Repeat(kind, inner) => self.emit_repeat(*kind, *inner, field, from),
            PatternKind::Alternative(alternatives) => {
                let to = self.new_state(GapClass::Any)?;
                for &alternative in alternatives {
                    let exit = self.emit_pattern(alternative, field, from)?;
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
        let to = self.new_state(GapClass::Any)?;
        self.states[from as usize].pattern_edges.push((matcher, to));
        Ok(to)
    }

    fn emit_repeat(
        &mut self,
        kind: QuantifierKind,
        inner: PatternId,
        field: Option<NodeFieldId>,
        from: State,
    ) -> Result<State, AnalysisLimit> {
        let exit = self.emit_pattern(inner, field, from)?;
        if kind == QuantifierKind::Optional {
            self.states[from as usize].eps_edges.push(exit);
            return Ok(exit);
        }

        self.states[exit as usize].eps_edges.push(from);
        let to = self.new_state(GapClass::Any)?;
        let bypass = if kind == QuantifierKind::ZeroOrMore {
            from
        } else {
            exit
        };
        self.states[bypass as usize].eps_edges.push(to);
        Ok(to)
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
            self.states[from as usize]
                .pattern_edges
                .push((ChildMatcher::new(KindConstraint::AnyNode, None), from));
            return Ok(from);
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
