//! Virtual machine for executing compiled Plotnik queries.

use arborium_tree_sitter::{Node, Tree};

use crate::bytecode::{
    Call, EffectOp, EffectOpcode, Entrypoint, InstructionView, MatchView, Module, Nav, StepAddr,
    Trampoline,
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

/// Derive skip policy from navigation mode without navigating.
/// Used when retrying a Call to determine the policy for the next checkpoint.
fn skip_policy_for_nav(nav: Nav) -> Option<SkipPolicy> {
    match nav {
        Nav::Down | Nav::Next => Some(SkipPolicy::Any),
        Nav::DownSkip | Nav::NextSkip => Some(SkipPolicy::Trivia),
        Nav::DownExact | Nav::NextExact => Some(SkipPolicy::Exact),
        // Stay doesn't navigate, Up doesn't retry among siblings
        _ => None,
    }
}
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

    /// When true, the next Call instruction should skip navigation (use Stay).
    /// This is set when backtracking to a Call retry checkpoint after advancing
    /// the cursor to a new sibling. The Call's navigation was already done, and
    /// we're now at the correct position for the callee to match.
    skip_call_nav: bool,

    /// Suppression depth counter. When > 0, effects are suppressed (not emitted to log).
    /// Incremented by SuppressBegin, decremented by SuppressEnd.
    suppress_depth: u16,

    /// Target address for Trampoline instruction.
    /// Set from entrypoint before execution; Trampoline jumps to this address.
    entrypoint_target: u16,
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
            skip_call_nav: false,
            suppress_depth: 0,
            entrypoint_target: 0,
        }
    }

    /// Execute query from entrypoint, returning effect log.
    ///
    /// This is a convenience method that uses `NoopTracer`, which gets
    /// completely optimized away at compile time.
    pub fn execute(
        self,
        module: &Module,
        bootstrap: StepAddr,
        entrypoint: &Entrypoint,
    ) -> Result<EffectLog<'t>, RuntimeError> {
        self.execute_with(module, bootstrap, entrypoint, &mut NoopTracer)
    }

    /// Execute query with a tracer for debugging.
    ///
    /// The tracer is generic, so `NoopTracer` calls are optimized away
    /// while `PrintTracer` calls collect execution trace.
    ///
    /// `bootstrap` is the preamble entry point - caller decides which preamble to use.
    pub fn execute_with<T: Tracer>(
        mut self,
        module: &Module,
        bootstrap: StepAddr,
        entrypoint: &Entrypoint,
        tracer: &mut T,
    ) -> Result<EffectLog<'t>, RuntimeError> {
        // Bootstrap address: where VM starts execution (preamble entry point).
        // Caller provides this, enabling different preamble types (root-match, recursive, etc.).
        self.ip = bootstrap;
        self.entrypoint_target = entrypoint.target;
        tracer.trace_enter_preamble();

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
                InstructionView::Trampoline(t) => self.exec_trampoline(t, tracer),
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
                skip_policy: None,
                suppress_depth: self.suppress_depth,
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

        // Get skip policy: from navigation (normal) or from nav mode (retry)
        let skip_policy = if self.skip_call_nav {
            // Retry: skip navigation, just check field, derive policy from nav mode
            self.skip_call_nav = false;
            self.check_field(c.node_field, tracer)?;
            skip_policy_for_nav(c.nav)
        } else {
            // Normal: navigate and capture skip policy
            self.navigate_to_field_with_policy(c.nav, c.node_field, tracer)?
        };

        // Push checkpoint for retry (both normal and retry paths need this)
        if let Some(policy) = skip_policy
            && policy != SkipPolicy::Exact
        {
            self.checkpoints.push(Checkpoint {
                descendant_index: self.cursor.descendant_index(),
                effect_watermark: self.effects.len(),
                frame_index: self.frames.current(),
                recursion_depth: self.recursion_depth,
                ip: self.ip,
                skip_policy: Some(policy),
                suppress_depth: self.suppress_depth,
            });
            tracer.trace_checkpoint_created(self.ip);
        }

        // Save tree depth AFTER navigation. On Return, we go up to this depth.
        let saved_depth = self.cursor.depth();
        tracer.trace_call(c.target.get());
        self.frames.push(c.next.get(), saved_depth);
        self.recursion_depth += 1;
        self.ip = c.target.get();
        Ok(())
    }

    /// Execute a Trampoline instruction.
    ///
    /// Trampoline is like Call, but the target comes from VM context (entrypoint_target)
    /// rather than being encoded in the instruction. Used for universal entry preamble.
    fn exec_trampoline<T: Tracer>(
        &mut self,
        t: Trampoline,
        tracer: &mut T,
    ) -> Result<(), RuntimeError> {
        if self.recursion_depth >= self.limits.recursion_limit {
            return Err(RuntimeError::RecursionLimitExceeded(self.recursion_depth));
        }

        // Trampoline doesn't navigate - it's always at root, cursor stays at root
        let saved_depth = self.cursor.depth();
        tracer.trace_call(self.entrypoint_target);
        self.frames.push(t.next.get(), saved_depth);
        self.recursion_depth += 1;
        self.ip = self.entrypoint_target;
        Ok(())
    }

    /// Navigate to a field and return the skip policy for retry support.
    ///
    /// Returns `Some(policy)` if navigation was performed, `None` if Stay nav was used.
    fn navigate_to_field_with_policy<T: Tracer>(
        &mut self,
        nav: Nav,
        field: Option<std::num::NonZeroU16>,
        tracer: &mut T,
    ) -> Result<Option<SkipPolicy>, RuntimeError> {
        if nav == Nav::Stay || nav == Nav::StayExact {
            self.check_field(field, tracer)?;
            return Ok(None);
        }

        let Some(policy) = self.cursor.navigate(nav) else {
            return Err(self.backtrack(tracer).unwrap_err());
        };
        tracer.trace_nav(nav, self.cursor.node());

        let Some(field_id) = field else {
            return Ok(Some(policy));
        };

        let cont_nav = continuation_nav(nav);
        loop {
            if self.cursor.field_id() == Some(field_id) {
                tracer.trace_field_success(field_id);
                return Ok(Some(policy));
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

        let (return_addr, saved_depth) = self.frames.pop();
        self.recursion_depth -= 1;

        // Prune frames (O(1) amortized)
        self.frames.prune(self.checkpoints.max_frame_ref());

        // Set matched_node BEFORE going up so effects after
        // a Call can capture the node that the callee matched.
        self.matched_node = Some(self.cursor.node());

        // Go up to saved depth level. This preserves sibling advances
        // (continue_search at same level) while restoring level when
        // the callee descended into children.
        while self.cursor.depth() > saved_depth {
            if !self.cursor.goto_parent() {
                break;
            }
        }

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
        self.suppress_depth = cp.suppress_depth;

        // Call retry: advance cursor to next sibling before re-executing
        if let Some(policy) = cp.skip_policy {
            if !self.cursor.continue_search(policy) {
                return self.backtrack(tracer);
            }
            tracer.trace_nav(continuation_nav(Nav::Down), self.cursor.node());
            self.skip_call_nav = true;
        }

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
            // Suppress control: trace then update depth
            SuppressBegin => {
                tracer.trace_suppress_control(SuppressBegin, self.suppress_depth > 0);
                self.suppress_depth += 1;
                return;
            }
            SuppressEnd => {
                self.suppress_depth = self.suppress_depth.saturating_sub(1);
                tracer.trace_suppress_control(SuppressEnd, self.suppress_depth > 0);
                return;
            }

            // Skip data effects when suppressing, but trace them
            _ if self.suppress_depth > 0 => {
                tracer.trace_effect_suppressed(op.opcode, op.payload);
                return;
            }

            // Data effects
            Node => {
                RuntimeEffect::Node(self.matched_node.expect("Node effect without matched_node"))
            }
            Text => {
                RuntimeEffect::Text(self.matched_node.expect("Text effect without matched_node"))
            }
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
