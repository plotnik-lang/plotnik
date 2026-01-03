//! Virtual machine for executing compiled Plotnik queries.

use arborium_tree_sitter::{Node, Tree};

use crate::bytecode::{
    Call, EffectOp, EffectOpcode, Entrypoint, InstructionView, MatchView, Module, Nav,
};

/// Get the nav for continue_search (always a sibling move).
fn continuation_nav(nav: Nav) -> Nav {
    match nav {
        Nav::Down | Nav::Next => Nav::Next,
        Nav::DownSkip | Nav::NextSkip => Nav::NextSkip,
        Nav::DownExact | Nav::NextExact => Nav::NextExact,
        // Up/Stay don't have search loops
        _ => Nav::Next,
    }
}

use super::checkpoint::{Checkpoint, CheckpointStack};
use super::cursor::{CursorWrapper, SkipPolicy};
use super::effect::{EffectLog, RuntimeEffect};
use super::error::RuntimeError;
use super::frame::FrameArena;
use super::trace::{NoopTracer, Tracer};

/// Runtime limits for query execution.
#[derive(Clone, Copy, Debug)]
pub struct FuelLimits {
    /// Maximum total steps (default: 1,000,000).
    pub exec_fuel: u32,
    /// Maximum call depth (default: 1,024).
    pub recursion_limit: u32,
}

impl Default for FuelLimits {
    fn default() -> Self {
        Self {
            exec_fuel: 1_000_000,
            recursion_limit: 1024,
        }
    }
}

/// Virtual machine state for query execution.
pub struct VM<'t> {
    cursor: CursorWrapper<'t>,
    /// Current instruction pointer (raw u16, 0 is valid at runtime).
    ip: u16,
    frames: FrameArena,
    checkpoints: CheckpointStack,
    effects: EffectLog<'t>,
    matched_node: Option<Node<'t>>,

    // Fuel tracking
    exec_fuel: u32,
    recursion_depth: u32,
    limits: FuelLimits,
}

impl<'t> VM<'t> {
    /// Create a new VM for execution.
    pub fn new(tree: &'t Tree, trivia_types: Vec<u16>, limits: FuelLimits) -> Self {
        Self {
            cursor: CursorWrapper::new(tree.walk(), trivia_types),
            ip: 0,
            frames: FrameArena::new(),
            checkpoints: CheckpointStack::new(),
            effects: EffectLog::new(),
            matched_node: None,
            exec_fuel: limits.exec_fuel,
            recursion_depth: 0,
            limits,
        }
    }

    /// Execute query from entrypoint, returning effect log.
    ///
    /// This is a convenience method that uses `NoopTracer`, which gets
    /// completely optimized away at compile time.
    pub fn execute(
        self,
        module: &Module,
        entrypoint: &Entrypoint,
    ) -> Result<EffectLog<'t>, RuntimeError> {
        self.execute_with(module, entrypoint, &mut NoopTracer)
    }

    /// Execute query with a tracer for debugging.
    ///
    /// The tracer is generic, so `NoopTracer` calls are optimized away
    /// while `PrintTracer` calls collect execution trace.
    pub fn execute_with<T: Tracer>(
        mut self,
        module: &Module,
        entrypoint: &Entrypoint,
        tracer: &mut T,
    ) -> Result<EffectLog<'t>, RuntimeError> {
        self.ip = entrypoint.target.get();
        tracer.trace_enter_entrypoint(self.ip);

        loop {
            // Fuel check
            if self.exec_fuel == 0 {
                return Err(RuntimeError::ExecFuelExhausted(self.limits.exec_fuel));
            }
            self.exec_fuel -= 1;

            // Fetch and dispatch
            let instr = module.decode_step(self.ip);
            tracer.trace_instruction(self.ip, &instr);

            let result = match instr {
                InstructionView::Match(m) => self.exec_match(m, tracer),
                InstructionView::Call(c) => self.exec_call(c, tracer),
                InstructionView::Return(_) => self.exec_return(tracer),
            };

            match result {
                Ok(()) | Err(RuntimeError::Backtracked) => continue,
                Err(RuntimeError::Accept) => return Ok(self.effects),
                Err(e) => return Err(e),
            }
        }
    }

    fn exec_match<T: Tracer>(
        &mut self,
        m: MatchView<'_>,
        tracer: &mut T,
    ) -> Result<(), RuntimeError> {
        for effect_op in m.pre_effects() {
            self.emit_effect(effect_op, tracer);
        }

        // Only clear matched_node for non-epsilon transitions.
        // For epsilon, preserve matched_node from previous match or return.
        if !m.is_epsilon() {
            self.matched_node = None;
            self.navigate_and_match(m, tracer)?;
        }

        for effect_op in m.post_effects() {
            self.emit_effect(effect_op, tracer);
        }

        self.branch_to_successors(m, tracer)
    }

    fn navigate_and_match<T: Tracer>(
        &mut self,
        m: MatchView<'_>,
        tracer: &mut T,
    ) -> Result<(), RuntimeError> {
        let Some(policy) = self.cursor.navigate(m.nav) else {
            return self.backtrack(tracer);
        };
        tracer.trace_nav(m.nav, self.cursor.node());

        let cont_nav = continuation_nav(m.nav);
        loop {
            if !self.node_matches(m, tracer) {
                self.advance_or_backtrack(policy, cont_nav, tracer)?;
                continue;
            }
            break;
        }

        tracer.trace_match_success(self.cursor.node());
        if let Some(field_id) = m.node_field {
            tracer.trace_field_success(field_id);
        }

        self.matched_node = Some(self.cursor.node());

        for field_id in m.neg_fields() {
            if self.cursor.node().child_by_field_id(field_id).is_some() {
                return self.backtrack(tracer);
            }
        }

        Ok(())
    }

    /// Check if current node matches type and field constraints.
    fn node_matches<T: Tracer>(&self, m: MatchView<'_>, tracer: &mut T) -> bool {
        if let Some(expected) = m.node_type
            && self.cursor.node().kind_id() != expected.get()
        {
            tracer.trace_match_failure(self.cursor.node());
            return false;
        }
        if let Some(expected) = m.node_field
            && self.cursor.field_id() != Some(expected)
        {
            tracer.trace_field_failure(self.cursor.node());
            return false;
        }
        true
    }

    fn branch_to_successors<T: Tracer>(
        &mut self,
        m: MatchView<'_>,
        tracer: &mut T,
    ) -> Result<(), RuntimeError> {
        if m.succ_count() == 0 {
            return Err(RuntimeError::Accept);
        }

        // Push checkpoints for alternate branches (in reverse order)
        for i in (1..m.succ_count()).rev() {
            self.checkpoints.push(Checkpoint {
                descendant_index: self.cursor.descendant_index(),
                effect_watermark: self.effects.len(),
                frame_index: self.frames.current(),
                recursion_depth: self.recursion_depth,
                ip: m.successor(i).get(),
            });
            tracer.trace_checkpoint_created(self.ip);
        }

        self.ip = m.successor(0).get();
        Ok(())
    }

    fn exec_call<T: Tracer>(&mut self, c: Call, tracer: &mut T) -> Result<(), RuntimeError> {
        if self.recursion_depth >= self.limits.recursion_limit {
            return Err(RuntimeError::RecursionLimitExceeded(self.recursion_depth));
        }

        self.navigate_to_field(c.nav, c.node_field, tracer)?;

        tracer.trace_call(c.target.get());
        self.frames.push(c.next.get());
        self.recursion_depth += 1;
        self.ip = c.target.get();
        Ok(())
    }

    fn navigate_to_field<T: Tracer>(
        &mut self,
        nav: Nav,
        field: Option<std::num::NonZeroU16>,
        tracer: &mut T,
    ) -> Result<(), RuntimeError> {
        if nav == Nav::Stay {
            return self.check_field(field, tracer);
        }

        let Some(policy) = self.cursor.navigate(nav) else {
            return self.backtrack(tracer);
        };
        tracer.trace_nav(nav, self.cursor.node());

        let Some(field_id) = field else {
            return Ok(());
        };

        let cont_nav = continuation_nav(nav);
        loop {
            if self.cursor.field_id() == Some(field_id) {
                tracer.trace_field_success(field_id);
                return Ok(());
            }
            tracer.trace_field_failure(self.cursor.node());
            self.advance_or_backtrack(policy, cont_nav, tracer)?;
        }
    }

    fn check_field<T: Tracer>(
        &mut self,
        field: Option<std::num::NonZeroU16>,
        tracer: &mut T,
    ) -> Result<(), RuntimeError> {
        let Some(field_id) = field else {
            return Ok(());
        };
        if self.cursor.field_id() != Some(field_id) {
            tracer.trace_field_failure(self.cursor.node());
            return self.backtrack(tracer);
        }
        tracer.trace_field_success(field_id);
        Ok(())
    }

    fn exec_return<T: Tracer>(&mut self, tracer: &mut T) -> Result<(), RuntimeError> {
        tracer.trace_return();

        // If no frames, we're returning from top-level entrypoint → Accept
        if self.frames.is_empty() {
            return Err(RuntimeError::Accept);
        }

        let return_addr = self.frames.pop();
        self.recursion_depth -= 1;

        // Prune frames (O(1) amortized)
        self.frames.prune(self.checkpoints.max_frame_ref());

        // Set matched_node to current cursor position so effects after
        // a Call can capture the node that the callee matched.
        self.matched_node = Some(self.cursor.node());

        self.ip = return_addr;
        Ok(())
    }

    fn backtrack<T: Tracer>(&mut self, tracer: &mut T) -> Result<(), RuntimeError> {
        let cp = self.checkpoints.pop().ok_or(RuntimeError::NoMatch)?;
        tracer.trace_backtrack();
        self.cursor.goto_descendant(cp.descendant_index);
        self.effects.truncate(cp.effect_watermark);
        self.frames.restore(cp.frame_index);
        self.recursion_depth = cp.recursion_depth;
        self.ip = cp.ip;
        Err(RuntimeError::Backtracked)
    }

    /// Advance to next sibling or backtrack if search exhausted.
    fn advance_or_backtrack<T: Tracer>(
        &mut self,
        policy: SkipPolicy,
        cont_nav: Nav,
        tracer: &mut T,
    ) -> Result<(), RuntimeError> {
        if !self.cursor.continue_search(policy) {
            return self.backtrack(tracer);
        }
        tracer.trace_nav(cont_nav, self.cursor.node());
        Ok(())
    }

    fn emit_effect<T: Tracer>(&mut self, op: EffectOp, tracer: &mut T) {
        use EffectOpcode::*;
        let effect = match op.opcode {
            Node => RuntimeEffect::Node(
                self.matched_node
                    .expect("Node effect without matched_node"),
            ),
            Text => RuntimeEffect::Text(
                self.matched_node
                    .expect("Text effect without matched_node"),
            ),
            Arr => RuntimeEffect::Arr,
            Push => RuntimeEffect::Push,
            EndArr => RuntimeEffect::EndArr,
            Obj => RuntimeEffect::Obj,
            EndObj => RuntimeEffect::EndObj,
            Set => RuntimeEffect::Set(op.payload as u16),
            Enum => RuntimeEffect::Enum(op.payload as u16),
            EndEnum => RuntimeEffect::EndEnum,
            Clear => RuntimeEffect::Clear,
            Null => RuntimeEffect::Null,
        };
        tracer.trace_effect(&effect);
        self.effects.push(effect);
    }
}
