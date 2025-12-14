//! The core query interpreter.
//!
//! Executes a compiled query against a tree-sitter AST, producing an effect stream
//! that can be materialized into a structured value.
//!
//! See ADR-0006 for detailed execution semantics.

use std::collections::HashSet;

use tree_sitter::{Node, TreeCursor};

use crate::ir::{
    CompiledQuery, EffectOp, Matcher, Nav, NavKind, NodeFieldId, NodeTypeId, RefTransition,
    TransitionId,
};

use super::effect_stream::EffectStream;
use super::error::RuntimeError;
use super::materializer::Materializer;
use super::value::Value;

/// A saved execution state for backtracking.
#[derive(Debug, Clone)]
struct Checkpoint {
    /// Tree-sitter descendant index for cursor restoration.
    cursor_checkpoint: usize,
    /// Number of ops in effect stream at save time.
    effect_ops_watermark: usize,
    /// Number of nodes in effect stream at save time.
    effect_nodes_watermark: usize,
    /// Current frame index at save time.
    recursion_frame: Option<u32>,
    /// Previous max_frame_watermark (for O(1) restore).
    prev_max_watermark: Option<u32>,
    /// Source transition for alternatives.
    transition_id: TransitionId,
    /// Index of next alternative to try.
    next_alt: u32,
}

/// Stack of checkpoints with O(1) watermark maintenance.
#[derive(Debug, Default)]
struct CheckpointStack {
    points: Vec<Checkpoint>,
    /// Highest frame index referenced by any checkpoint.
    max_frame_watermark: Option<u32>,
}

impl CheckpointStack {
    fn new() -> Self {
        Self::default()
    }

    fn push(&mut self, mut point: Checkpoint) {
        point.prev_max_watermark = self.max_frame_watermark;
        if let Some(frame) = point.recursion_frame {
            self.max_frame_watermark = Some(match self.max_frame_watermark {
                Some(max) => max.max(frame),
                None => frame,
            });
        }
        self.points.push(point);
    }

    fn pop(&mut self) -> Option<Checkpoint> {
        let point = self.points.pop()?;
        self.max_frame_watermark = point.prev_max_watermark;
        Some(point)
    }
}

/// A call frame for definition references.
#[derive(Debug, Clone)]
struct Frame {
    /// Index of caller's frame (None if called from top level).
    parent: Option<u32>,
    /// Ref ID to verify Exit matches Enter.
    ref_id: u16,
    /// Transition that entered this call (to retrieve returns via successors()[1..]).
    enter_transition: TransitionId,
}

/// Append-only arena of call frames.
#[derive(Debug, Default)]
struct FrameArena {
    frames: Vec<Frame>,
    /// Index of current frame (the "stack pointer").
    current: Option<u32>,
}

impl FrameArena {
    fn new() -> Self {
        Self::default()
    }

    /// Push a new frame, returns its index.
    fn push(&mut self, parent: Option<u32>, ref_id: u16, enter_transition: TransitionId) -> u32 {
        let idx = self.frames.len() as u32;
        self.frames.push(Frame {
            parent,
            ref_id,
            enter_transition,
        });
        self.current = Some(idx);
        idx
    }

    /// Get current frame.
    fn current_frame(&self) -> Option<&Frame> {
        self.current.map(|idx| &self.frames[idx as usize])
    }

    /// Exit current frame (set current to parent).
    fn exit(&mut self) -> Option<&Frame> {
        let frame = self.current_frame()?;
        let parent = frame.parent;
        let idx = self.current?;
        self.current = parent;
        Some(&self.frames[idx as usize])
    }

    /// Prune frames above the high-water mark.
    fn prune(&mut self, checkpoints: &CheckpointStack) {
        let high_water = match (self.current, checkpoints.max_frame_watermark) {
            (None, None) => return,
            (Some(c), None) => c,
            (None, Some(m)) => m,
            (Some(c), Some(m)) => c.max(m),
        };
        self.frames.truncate((high_water + 1) as usize);
    }
}

/// Default execution fuel (transitions).
const DEFAULT_EXEC_FUEL: u32 = 1_000_000;
/// Default recursion fuel (Enter operations).
const DEFAULT_RECURSION_FUEL: u32 = 1024;

/// Query interpreter that executes a compiled query against an AST.
pub struct QueryInterpreter<'q, 'tree> {
    query: &'q CompiledQuery,
    cursor: TreeCursor<'tree>,
    source: &'tree str,
    checkpoints: CheckpointStack,
    frames: FrameArena,
    effects: EffectStream<'tree>,
    /// Trivia node type IDs (for skip-trivia navigation).
    trivia_kinds: HashSet<NodeTypeId>,
    /// Matched node slot (cleared at start of each transition).
    matched_node: Option<Node<'tree>>,
    /// Execution fuel remaining.
    exec_fuel: u32,
    /// Recursion fuel remaining.
    recursion_fuel: u32,
}

impl<'q, 'tree> QueryInterpreter<'q, 'tree> {
    /// Creates a new interpreter.
    ///
    /// The cursor should be positioned at the tree root.
    pub fn new(query: &'q CompiledQuery, cursor: TreeCursor<'tree>, source: &'tree str) -> Self {
        let trivia_kinds: HashSet<_> = query.trivia_kinds().iter().copied().collect();
        Self {
            query,
            cursor,
            source,
            checkpoints: CheckpointStack::new(),
            frames: FrameArena::new(),
            effects: EffectStream::new(),
            trivia_kinds,
            matched_node: None,
            exec_fuel: DEFAULT_EXEC_FUEL,
            recursion_fuel: DEFAULT_RECURSION_FUEL,
        }
    }

    /// Set execution fuel limit.
    pub fn with_exec_fuel(mut self, fuel: u32) -> Self {
        self.exec_fuel = fuel;
        self
    }

    /// Set recursion fuel limit.
    pub fn with_recursion_fuel(mut self, fuel: u32) -> Self {
        self.recursion_fuel = fuel;
        self
    }

    /// Run the query and return the result.
    pub fn run(mut self) -> Result<Value<'tree>, RuntimeError> {
        // Start at transition 0 (default entrypoint)
        let start_transition = 0;

        match self.execute(start_transition) {
            Ok(true) => Ok(Materializer::materialize(&self.effects)),
            Ok(false) => Ok(Value::Null), // No match
            Err(e) => Err(e),
        }
    }

    /// Execute from a given transition, returns true if matched.
    fn execute(&mut self, start: TransitionId) -> Result<bool, RuntimeError> {
        let mut current = start;

        loop {
            // Check fuel
            if self.exec_fuel == 0 {
                return Err(RuntimeError::ExecFuelExhausted);
            }
            self.exec_fuel -= 1;

            // Clear matched_node slot at start of each transition
            self.matched_node = None;

            let view = self.query.transition_view(current);
            let nav = view.nav();
            let matcher = view.matcher();
            let ref_marker = view.ref_marker();
            let successors = view.successors();

            // Step 1: Execute navigation
            let nav_ok = self.execute_nav(nav);
            if !nav_ok {
                // Navigation failed, backtrack
                if let Some(next) = self.backtrack()? {
                    current = next;
                    continue;
                }
                return Ok(false);
            }

            // Step 2: Try matcher (with skip policy from nav)
            let match_ok = self.execute_matcher(matcher, nav);
            if !match_ok {
                // Match failed, backtrack
                if let Some(next) = self.backtrack()? {
                    current = next;
                    continue;
                }
                return Ok(false);
            }

            // Step 3: Execute effects
            for &effect in view.effects() {
                self.execute_effect(effect);
            }

            // Step 4: Process ref_marker
            match ref_marker {
                RefTransition::None => {}
                RefTransition::Enter(ref_id) => {
                    if self.recursion_fuel == 0 {
                        return Err(RuntimeError::RecursionLimitExceeded);
                    }
                    self.recursion_fuel -= 1;

                    // Push frame with returns = successors[1..]
                    self.frames.push(self.frames.current, ref_id, current);

                    // Jump to definition entry = successors[0]
                    if successors.is_empty() {
                        panic!("Enter transition must have at least one successor");
                    }
                    current = successors[0];
                    continue;
                }
                RefTransition::Exit(ref_id) => {
                    // Verify ref_id matches
                    let frame = self.frames.current_frame().expect("Exit without frame");
                    assert_eq!(frame.ref_id, ref_id, "Exit ref_id mismatch");

                    // Get returns from enter transition
                    let enter_trans = frame.enter_transition;
                    let enter_view = self.query.transition_view(enter_trans);
                    let returns = &enter_view.successors()[1..];

                    // Pop frame
                    self.frames.exit();

                    // Prune frames if possible
                    self.frames.prune(&self.checkpoints);

                    // Continue with returns as successors
                    if returns.is_empty() {
                        // Definition matched, no returns = we're done with this path
                        // This shouldn't happen in well-formed graphs
                        if let Some(next) = self.backtrack()? {
                            current = next;
                            continue;
                        }
                        return Ok(true);
                    }

                    // Save checkpoint for alternatives if multiple returns
                    if returns.len() > 1 {
                        self.save_checkpoint(enter_trans, 2); // Skip successors[0] and [1]
                    }

                    current = returns[0];
                    continue;
                }
            }

            // Step 5: Process successors
            if successors.is_empty() {
                // Terminal transition - match succeeded
                return Ok(true);
            }

            // Save checkpoint for alternatives
            if successors.len() > 1 {
                self.save_checkpoint(current, 1);
            }

            current = successors[0];
        }
    }

    /// Save a checkpoint for backtracking.
    fn save_checkpoint(&mut self, transition_id: TransitionId, next_alt: u32) {
        let checkpoint = Checkpoint {
            cursor_checkpoint: self.cursor.descendant_index(),
            effect_ops_watermark: self.effects.ops().len(),
            effect_nodes_watermark: self.effects.nodes().len(),
            recursion_frame: self.frames.current,
            prev_max_watermark: None, // Set by CheckpointStack::push
            transition_id,
            next_alt,
        };
        self.checkpoints.push(checkpoint);
    }

    /// Backtrack to the next alternative. Returns the transition to try.
    fn backtrack(&mut self) -> Result<Option<TransitionId>, RuntimeError> {
        loop {
            let Some(mut checkpoint) = self.checkpoints.pop() else {
                return Ok(None);
            };

            // Restore cursor
            self.cursor.goto_descendant(checkpoint.cursor_checkpoint);

            // Restore effects
            self.effects.truncate(
                checkpoint.effect_ops_watermark,
                checkpoint.effect_nodes_watermark,
            );

            // Restore frame
            self.frames.current = checkpoint.recursion_frame;

            // Get next alternative
            let view = self.query.transition_view(checkpoint.transition_id);
            let successors = view.successors();

            if (checkpoint.next_alt as usize) < successors.len() {
                let next = successors[checkpoint.next_alt as usize];
                checkpoint.next_alt += 1;

                // Re-save if more alternatives remain
                if (checkpoint.next_alt as usize) < successors.len() {
                    self.checkpoints.push(checkpoint);
                }

                return Ok(Some(next));
            }
            // No more alternatives at this checkpoint, try next
        }
    }

    /// Execute navigation, returns true if successful.
    fn execute_nav(&mut self, nav: Nav) -> bool {
        match nav.kind {
            NavKind::Stay => true,

            NavKind::Next => self.cursor.goto_next_sibling(),

            NavKind::NextSkipTrivia => {
                while self.cursor.goto_next_sibling() {
                    if !self.is_trivia(self.cursor.node()) {
                        return true;
                    }
                }
                false
            }

            NavKind::NextExact => self.cursor.goto_next_sibling(),

            NavKind::Down => self.cursor.goto_first_child(),

            NavKind::DownSkipTrivia => {
                if !self.cursor.goto_first_child() {
                    return false;
                }
                while self.is_trivia(self.cursor.node()) {
                    if !self.cursor.goto_next_sibling() {
                        return false;
                    }
                }
                true
            }

            NavKind::DownExact => self.cursor.goto_first_child(),

            NavKind::Up => {
                for _ in 0..nav.level {
                    if !self.cursor.goto_parent() {
                        return false;
                    }
                }
                true
            }

            NavKind::UpSkipTrivia => {
                // Validate we're at last non-trivia child before ascending
                let current_id = self.cursor.node().id();
                if let Some(parent) = self.cursor.node().parent() {
                    let child_count = parent.child_count() as u32;
                    let mut found_current = false;
                    for i in 0..child_count {
                        if let Some(child) = parent.child(i) {
                            if child.id() == current_id {
                                found_current = true;
                                continue;
                            }
                            if found_current && !self.is_trivia(child) {
                                return false;
                            }
                        }
                    }
                }
                self.cursor.goto_parent()
            }

            NavKind::UpExact => {
                // Validate we're at last child
                let node = self.cursor.node();
                if let Some(parent) = node.parent() {
                    let child_count = parent.child_count();
                    if child_count > 0 {
                        let last_child = parent.child((child_count - 1) as u32);
                        if last_child.map(|c| c.id()) != Some(node.id()) {
                            return false;
                        }
                    }
                }
                self.cursor.goto_parent()
            }
        }
    }

    /// Execute matcher with skip policy, returns true if matched.
    fn execute_matcher(&mut self, matcher: &Matcher, nav: Nav) -> bool {
        match matcher {
            Matcher::Epsilon => true,

            Matcher::Node {
                kind,
                field,
                negated_fields,
            } => {
                let matched = self.try_match_node(*kind, *field, *negated_fields, true, nav);
                if matched {
                    self.matched_node = Some(self.cursor.node());
                }
                matched
            }

            Matcher::Anonymous {
                kind,
                field,
                negated_fields,
            } => {
                let matched = self.try_match_node(*kind, *field, *negated_fields, false, nav);
                if matched {
                    self.matched_node = Some(self.cursor.node());
                }
                matched
            }

            Matcher::Wildcard => {
                self.matched_node = Some(self.cursor.node());
                true
            }
        }
    }

    /// Try to match a node with the given constraints.
    fn try_match_node(
        &mut self,
        kind: NodeTypeId,
        field: Option<NodeFieldId>,
        negated_fields: crate::ir::Slice<NodeFieldId>,
        named: bool,
        nav: Nav,
    ) -> bool {
        // Determine skip policy
        let can_skip = match nav.kind {
            NavKind::Next | NavKind::Down => true,
            NavKind::NextSkipTrivia | NavKind::DownSkipTrivia => false, // Already handled trivia
            _ => false,
        };

        loop {
            let node = self.cursor.node();

            // Check named/anonymous
            if named != node.is_named() {
                if can_skip && self.cursor.goto_next_sibling() {
                    continue;
                }
                return false;
            }

            // Check kind
            if node.kind_id() != kind {
                if can_skip && self.cursor.goto_next_sibling() {
                    continue;
                }
                return false;
            }

            // Check field constraint
            if let Some(field_id) = field {
                let actual_field = self.cursor.field_id();
                if actual_field != Some(field_id) {
                    if can_skip && self.cursor.goto_next_sibling() {
                        continue;
                    }
                    return false;
                }
            }

            // Check negated fields
            let neg_fields = self.query.resolve_negated_fields(negated_fields);
            for &neg_field in neg_fields {
                if node.child_by_field_id(neg_field.get()).is_some() {
                    if can_skip && self.cursor.goto_next_sibling() {
                        continue;
                    }
                    return false;
                }
            }

            return true;
        }
    }

    /// Execute an effect operation.
    fn execute_effect(&mut self, effect: EffectOp) {
        self.effects.push_op(effect);

        if matches!(effect, EffectOp::CaptureNode) {
            let node = self.matched_node.expect("CaptureNode without matched node");
            self.effects.push_node(node, self.source);
        }
    }

    /// Check if a node is trivia.
    fn is_trivia(&self, node: Node) -> bool {
        self.trivia_kinds.contains(&node.kind_id())
    }
}
