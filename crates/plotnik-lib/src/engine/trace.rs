//! Tracing infrastructure for debugging VM execution.
//!
//! # Design: Zero-Cost Abstraction
//!
//! The tracer is designed as a zero-cost abstraction. When `NoopTracer` is used:
//! - All trait methods are `#[inline(always)]` empty functions
//! - The compiler eliminates all tracer calls and their arguments
//! - No tracing-related state exists in core execution structures
//!
//! # Design: Tracer-Owned State
//!
//! Tracing-only state (like checkpoint creation IPs for backtrack display) is
//! maintained by the tracer itself, not in core structures like `Checkpoint`.
//! This keeps execution structures minimal and avoids "spilling" tracing concerns
//! into `exec`. For example:
//! - `trace_checkpoint_created(ip)` - tracer pushes to its own stack
//! - `trace_backtrack()` - tracer pops its stack to get the display IP
//!
//! `NoopTracer` ignores these calls (optimized away), while `PrintTracer`
//! maintains parallel state for display purposes.

use std::num::NonZeroU16;

use arborium_tree_sitter::Node;

use crate::Colors;
use crate::bytecode::{
    EffectOpcode, InstructionView, LineBuilder, MatchView, Module, Nav, Symbol, cols,
    format_effect, trace, truncate_text, width_for_count,
};

use super::effect::RuntimeEffect;

/// Verbosity level for trace output.
///
/// Controls which sub-lines are shown and whether node text is included.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Verbosity {
    /// Default: match, backtrack, call/return. Kind only, no text.
    #[default]
    Default,
    /// Verbose (-v): all sub-lines. Text on match/failure.
    Verbose,
    /// Very verbose (-vv): all sub-lines. Text on everything including nav.
    VeryVerbose,
}

/// Tracer trait for VM execution instrumentation.
///
/// All methods receive raw data (IDs, nodes) that the VM already has.
/// Formatting and name resolution happen in the tracer implementation.
///
/// Each method is called at a specific point during execution:
/// - `trace_instruction` - before executing an instruction
/// - `trace_nav` - after navigation succeeds
/// - `trace_match_success/failure` - after type check
/// - `trace_field_success/failure` - after field check
/// - `trace_effect` - after emitting an effect
/// - `trace_call` - when entering a definition
/// - `trace_return` - when returning from a definition
/// - `trace_checkpoint_created` - when a checkpoint is pushed
/// - `trace_backtrack` - when restoring a checkpoint
/// - `trace_enter_entrypoint` - when entering an entrypoint (for labels)
pub trait Tracer {
    /// Called before executing an instruction.
    fn trace_instruction(&mut self, ip: u16, instr: &InstructionView<'_>);

    /// Called after navigation succeeds.
    fn trace_nav(&mut self, nav: Nav, node: Node<'_>);

    /// Called after type check succeeds.
    fn trace_match_success(&mut self, node: Node<'_>);

    /// Called after type check fails.
    fn trace_match_failure(&mut self, node: Node<'_>);

    /// Called after field check succeeds.
    fn trace_field_success(&mut self, field_id: NonZeroU16);

    /// Called after field check fails.
    fn trace_field_failure(&mut self, node: Node<'_>);

    /// Called after emitting an effect.
    fn trace_effect(&mut self, effect: &RuntimeEffect<'_>);

    /// Called when an effect is suppressed (inside @_ capture).
    fn trace_effect_suppressed(&mut self, opcode: EffectOpcode, payload: usize);

    /// Called for SuppressBegin/SuppressEnd control effects.
    /// `suppressed` is true if already inside another suppress scope.
    fn trace_suppress_control(&mut self, opcode: EffectOpcode, suppressed: bool);

    /// Called when entering a definition via Call.
    fn trace_call(&mut self, target_ip: u16);

    /// Called when returning from a definition.
    fn trace_return(&mut self);

    /// Called when a checkpoint is created.
    fn trace_checkpoint_created(&mut self, ip: u16);

    /// Called when backtracking occurs.
    fn trace_backtrack(&mut self);

    /// Called when entering an entrypoint (for section labels).
    fn trace_enter_entrypoint(&mut self, target_ip: u16);

    /// Called when entering the preamble (bootstrap wrapper).
    fn trace_enter_preamble(&mut self);
}

/// No-op tracer that gets optimized away completely.
pub struct NoopTracer;

impl Tracer for NoopTracer {
    #[inline(always)]
    fn trace_instruction(&mut self, _ip: u16, _instr: &InstructionView<'_>) {}

    #[inline(always)]
    fn trace_nav(&mut self, _nav: Nav, _node: Node<'_>) {}

    #[inline(always)]
    fn trace_match_success(&mut self, _node: Node<'_>) {}

    #[inline(always)]
    fn trace_match_failure(&mut self, _node: Node<'_>) {}

    #[inline(always)]
    fn trace_field_success(&mut self, _field_id: NonZeroU16) {}

    #[inline(always)]
    fn trace_field_failure(&mut self, _node: Node<'_>) {}

    #[inline(always)]
    fn trace_effect(&mut self, _effect: &RuntimeEffect<'_>) {}

    #[inline(always)]
    fn trace_effect_suppressed(&mut self, _opcode: EffectOpcode, _payload: usize) {}

    #[inline(always)]
    fn trace_suppress_control(&mut self, _opcode: EffectOpcode, _suppressed: bool) {}

    #[inline(always)]
    fn trace_call(&mut self, _target_ip: u16) {}

    #[inline(always)]
    fn trace_return(&mut self) {}

    #[inline(always)]
    fn trace_checkpoint_created(&mut self, _ip: u16) {}

    #[inline(always)]
    fn trace_backtrack(&mut self) {}

    #[inline(always)]
    fn trace_enter_entrypoint(&mut self, _target_ip: u16) {}

    #[inline(always)]
    fn trace_enter_preamble(&mut self) {}
}

use std::collections::BTreeMap;

/// Tracer that collects execution trace for debugging.
pub struct PrintTracer<'s> {
    /// Source code for extracting node text.
    source: &'s [u8],
    /// Verbosity level for output filtering.
    verbosity: Verbosity,
    /// Collected trace lines.
    lines: Vec<String>,
    /// Line builder for formatting.
    builder: LineBuilder,
    /// Maps node type ID to name.
    node_type_names: BTreeMap<u16, String>,
    /// Maps node field ID to name.
    node_field_names: BTreeMap<u16, String>,
    /// Maps member index to name (for Set/Enum effect display).
    member_names: Vec<String>,
    /// Maps entrypoint target IP to name (for labels and call/return).
    entrypoint_by_ip: BTreeMap<u16, String>,
    /// Parallel stack of checkpoint creation IPs (for backtrack display).
    checkpoint_ips: Vec<u16>,
    /// Stack of definition names (for return display).
    definition_stack: Vec<String>,
    /// Pending return instruction IP (for consolidated return line).
    pending_return_ip: Option<u16>,
    /// Step width for formatting.
    step_width: usize,
    /// Color palette.
    colors: Colors,
}

impl<'s> PrintTracer<'s> {
    pub fn new(source: &'s str, module: &Module, verbosity: Verbosity, colors: Colors) -> Self {
        let header = module.header();
        let strings = module.strings();
        let types = module.types();
        let node_types = module.node_types();
        let node_fields = module.node_fields();
        let entrypoints = module.entrypoints();

        let mut node_type_names = BTreeMap::new();
        for i in 0..node_types.len() {
            let t = node_types.get(i);
            node_type_names.insert(t.id, strings.get(t.name).to_string());
        }

        let mut node_field_names = BTreeMap::new();
        for i in 0..node_fields.len() {
            let f = node_fields.get(i);
            node_field_names.insert(f.id, strings.get(f.name).to_string());
        }

        // Build member names lookup (index → name)
        let member_names: Vec<String> = (0..types.members_count())
            .map(|i| strings.get(types.get_member(i).name).to_string())
            .collect();

        // Build entrypoint IP → name lookup
        let mut entrypoint_by_ip = BTreeMap::new();
        for i in 0..entrypoints.len() {
            let e = entrypoints.get(i);
            entrypoint_by_ip.insert(e.target.get(), strings.get(e.name).to_string());
        }

        let step_width = width_for_count(header.transitions_count as usize);

        Self {
            source: source.as_bytes(),
            verbosity,
            lines: Vec::new(),
            builder: LineBuilder::new(step_width),
            node_type_names,
            node_field_names,
            member_names,
            entrypoint_by_ip,
            checkpoint_ips: Vec::new(),
            definition_stack: Vec::new(),
            pending_return_ip: None,
            step_width,
            colors,
        }
    }

    fn node_type_name(&self, id: u16) -> &str {
        self.node_type_names.get(&id).map_or("?", |s| s.as_str())
    }

    fn node_field_name(&self, id: u16) -> &str {
        self.node_field_names.get(&id).map_or("?", |s| s.as_str())
    }

    fn member_name(&self, idx: u16) -> &str {
        self.member_names
            .get(idx as usize)
            .map_or("?", |s| s.as_str())
    }

    fn entrypoint_name(&self, ip: u16) -> &str {
        self.entrypoint_by_ip.get(&ip).map_or("?", |s| s.as_str())
    }

    /// Format kind with source text, dynamically truncated to fit content width.
    /// Text is displayed dimmed and green, no quotes.
    fn format_kind_with_text(&self, kind: &str, text: &str) -> String {
        let c = &self.colors;

        // Available content width = TOTAL_WIDTH - prefix_width + step_width
        // prefix_width = INDENT + step_width + GAP + SYMBOL + GAP = 9 + step_width
        // +step_width because ellipsis can extend into the successors column
        // (sub-lines have no successors, so we use that space)
        // This simplifies to: TOTAL_WIDTH - 9 = 35
        let available = cols::TOTAL_WIDTH - 9;

        // Text budget = available - kind.len() - 1 (space), minimum 12
        let text_budget = available.saturating_sub(kind.len() + 1).max(12);

        let truncated = truncate_text(text, text_budget);
        format!("{} {}{}{}{}", kind, c.dim, c.green, truncated, c.reset)
    }

    /// Format a runtime effect for display.
    fn format_effect(&self, effect: &RuntimeEffect<'_>) -> String {
        use RuntimeEffect::*;
        match effect {
            Node(_) => "Node".to_string(),
            Text(_) => "Text".to_string(),
            Arr => "Arr".to_string(),
            Push => "Push".to_string(),
            EndArr => "EndArr".to_string(),
            Obj => "Obj".to_string(),
            EndObj => "EndObj".to_string(),
            Set(idx) => format!("Set \"{}\"", self.member_name(*idx)),
            Enum(idx) => format!("Enum \"{}\"", self.member_name(*idx)),
            EndEnum => "EndEnum".to_string(),
            Clear => "Clear".to_string(),
            Null => "Null".to_string(),
        }
    }

    /// Format a suppressed effect from opcode and payload.
    fn format_effect_from_opcode(&self, opcode: EffectOpcode, payload: usize) -> String {
        use EffectOpcode::*;
        match opcode {
            Node => "Node".to_string(),
            Text => "Text".to_string(),
            Arr => "Arr".to_string(),
            Push => "Push".to_string(),
            EndArr => "EndArr".to_string(),
            Obj => "Obj".to_string(),
            EndObj => "EndObj".to_string(),
            Set => format!("Set \"{}\"", self.member_name(payload as u16)),
            Enum => format!("Enum \"{}\"", self.member_name(payload as u16)),
            EndEnum => "EndEnum".to_string(),
            Clear => "Clear".to_string(),
            Null => "Null".to_string(),
            SuppressBegin | SuppressEnd => unreachable!(),
        }
    }

    /// Format match content for instruction line (matches dump format exactly).
    ///
    /// Order: [pre-effects] !neg_fields field: (type) [post-effects]
    fn format_match_content(&self, m: &MatchView<'_>) -> String {
        let mut parts = Vec::new();

        // Pre-effects: [Effect1 Effect2]
        let pre: Vec<_> = m.pre_effects().map(|e| format_effect(&e)).collect();
        if !pre.is_empty() {
            parts.push(format!("[{}]", pre.join(" ")));
        }

        // Negated fields: !field1 !field2
        for field_id in m.neg_fields() {
            let name = self.node_field_name(field_id);
            parts.push(format!("!{name}"));
        }

        // Node pattern: field: (type) / (type) / field: _ / empty
        let node_part = self.format_node_pattern(m);
        if !node_part.is_empty() {
            parts.push(node_part);
        }

        // Post-effects: [Effect1 Effect2]
        let post: Vec<_> = m.post_effects().map(|e| format_effect(&e)).collect();
        if !post.is_empty() {
            parts.push(format!("[{}]", post.join(" ")));
        }

        parts.join(" ")
    }

    /// Format node pattern: `field: (type)` or `(type)` or `field: _` or empty.
    fn format_node_pattern(&self, m: &MatchView<'_>) -> String {
        let mut result = String::new();

        if let Some(f) = m.node_field {
            result.push_str(self.node_field_name(f.get()));
            result.push_str(": ");
        }

        if let Some(t) = m.node_type {
            result.push('(');
            result.push_str(self.node_type_name(t.get()));
            result.push(')');
        } else if m.node_field.is_some() {
            result.push('_');
        }

        result
    }

    /// Print all trace lines.
    pub fn print(&self) {
        for line in &self.lines {
            println!("{}", line);
        }
    }

    /// Add an instruction line.
    fn add_instruction(&mut self, ip: u16, symbol: Symbol, content: &str, successors: &str) {
        let prefix = format!("  {:0sw$} {} ", ip, symbol.format(), sw = self.step_width);
        let line = self
            .builder
            .pad_successors(format!("{prefix}{content}"), successors);
        self.lines.push(line);
    }

    /// Add a sub-line (blank step area + symbol + content).
    fn add_subline(&mut self, symbol: Symbol, content: &str) {
        let step_area = 2 + self.step_width + 1;
        let prefix = format!("{:step_area$}{} ", "", symbol.format());
        self.lines.push(format!("{prefix}{content}"));
    }

    /// Format definition name (blue). User definitions get parentheses, preamble doesn't.
    fn format_def_name(&self, name: &str) -> String {
        let c = self.colors;
        if name.starts_with('_') {
            // Preamble/internal names: no parentheses
            format!("{}{}{}", c.blue, name, c.reset)
        } else {
            // User definitions: wrap in parentheses
            format!("({}{}{})", c.blue, name, c.reset)
        }
    }

    /// Format definition label with colon (blue).
    fn format_def_label(&self, name: &str) -> String {
        let c = self.colors;
        format!("{}{}{}:", c.blue, name, c.reset)
    }

    /// Push a definition label, with empty line separator (except for first label).
    fn push_def_label(&mut self, name: &str) {
        if !self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.lines.push(self.format_def_label(name));
    }
}

impl Tracer for PrintTracer<'_> {
    fn trace_instruction(&mut self, ip: u16, instr: &InstructionView<'_>) {
        match instr {
            InstructionView::Match(m) => {
                // Show ε for epsilon transitions, empty otherwise (nav shown in sublines)
                let symbol = if m.is_epsilon() {
                    Symbol::EPSILON
                } else {
                    Symbol::EMPTY
                };
                let content = self.format_match_content(m);
                let successors = format_match_successors(m);
                self.add_instruction(ip, symbol, &content, &successors);
            }
            InstructionView::Call(c) => {
                let name = self.entrypoint_name(c.target.get());
                let content = self.format_def_name(name);
                let successors = format!("{:02} : {:02}", c.target.get(), c.next.get());
                self.add_instruction(ip, Symbol::EMPTY, &content, &successors);
            }
            InstructionView::Return(_) => {
                self.pending_return_ip = Some(ip);
            }
            InstructionView::Trampoline(t) => {
                // Trampoline shows as a call to the entrypoint target
                let content = "Trampoline";
                let successors = format!("{:02}", t.next.get());
                self.add_instruction(ip, Symbol::EMPTY, content, &successors);
            }
        }
    }

    fn trace_nav(&mut self, nav: Nav, node: Node<'_>) {
        // Navigation sub-lines hidden in default verbosity
        if self.verbosity == Verbosity::Default {
            return;
        }

        let kind = node.kind();
        let symbol = match nav {
            Nav::Down | Nav::DownSkip | Nav::DownExact => trace::NAV_DOWN,
            Nav::Next | Nav::NextSkip | Nav::NextExact => trace::NAV_NEXT,
            Nav::Up(_) | Nav::UpSkipTrivia(_) | Nav::UpExact(_) => trace::NAV_UP,
            Nav::Stay | Nav::StayExact => Symbol::EMPTY,
        };

        // Text only in VeryVerbose
        if self.verbosity == Verbosity::VeryVerbose {
            let text = node.utf8_text(self.source).unwrap_or("?");
            let content = self.format_kind_with_text(kind, text);
            self.add_subline(symbol, &content);
        } else {
            self.add_subline(symbol, kind);
        }
    }

    fn trace_match_success(&mut self, node: Node<'_>) {
        let kind = node.kind();

        // Text on match/failure in Verbose+
        if self.verbosity != Verbosity::Default {
            let text = node.utf8_text(self.source).unwrap_or("?");
            let content = self.format_kind_with_text(kind, text);
            self.add_subline(trace::MATCH_SUCCESS, &content);
        } else {
            self.add_subline(trace::MATCH_SUCCESS, kind);
        }
    }

    fn trace_match_failure(&mut self, node: Node<'_>) {
        let kind = node.kind();

        // Text on match/failure in Verbose+
        if self.verbosity != Verbosity::Default {
            let text = node.utf8_text(self.source).unwrap_or("?");
            let content = self.format_kind_with_text(kind, text);
            self.add_subline(trace::MATCH_FAILURE, &content);
        } else {
            self.add_subline(trace::MATCH_FAILURE, kind);
        }
    }

    fn trace_field_success(&mut self, field_id: NonZeroU16) {
        // Field success sub-lines hidden in default verbosity
        if self.verbosity == Verbosity::Default {
            return;
        }

        let name = self.node_field_name(field_id.get());
        self.add_subline(trace::MATCH_SUCCESS, &format!("{}:", name));
    }

    fn trace_field_failure(&mut self, _node: Node<'_>) {
        // Field failures are silent - we just backtrack
    }

    fn trace_effect(&mut self, effect: &RuntimeEffect<'_>) {
        // Effect sub-lines hidden in default verbosity
        if self.verbosity == Verbosity::Default {
            return;
        }

        let effect_str = self.format_effect(effect);
        self.add_subline(trace::EFFECT, &effect_str);
    }

    fn trace_effect_suppressed(&mut self, opcode: EffectOpcode, payload: usize) {
        // Effect sub-lines hidden in default verbosity
        if self.verbosity == Verbosity::Default {
            return;
        }

        let effect_str = self.format_effect_from_opcode(opcode, payload);
        self.add_subline(trace::EFFECT_SUPPRESSED, &effect_str);
    }

    fn trace_suppress_control(&mut self, opcode: EffectOpcode, suppressed: bool) {
        // Effect sub-lines hidden in default verbosity
        if self.verbosity == Verbosity::Default {
            return;
        }

        let name = match opcode {
            EffectOpcode::SuppressBegin => "SuppressBegin",
            EffectOpcode::SuppressEnd => "SuppressEnd",
            _ => unreachable!(),
        };
        let symbol = if suppressed {
            trace::EFFECT_SUPPRESSED
        } else {
            trace::EFFECT
        };
        self.add_subline(symbol, name);
    }

    fn trace_call(&mut self, target_ip: u16) {
        let name = self.entrypoint_name(target_ip).to_string();
        self.add_subline(trace::CALL, &self.format_def_name(&name));
        self.push_def_label(&name);
        self.definition_stack.push(name);
    }

    fn trace_return(&mut self) {
        let ip = self
            .pending_return_ip
            .take()
            .expect("trace_return without trace_instruction");
        let name = self.definition_stack.pop().unwrap_or_default();
        let content = self.format_def_name(&name);
        // Show ◼ when returning from top-level (stack now empty)
        let is_top_level = self.definition_stack.is_empty();
        let successor = if is_top_level { "◼" } else { "" };
        self.add_instruction(ip, trace::RETURN, &content, successor);
        // Print caller's label after return (if not top-level)
        if let Some(caller) = self.definition_stack.last().cloned() {
            self.push_def_label(&caller);
        }
    }

    fn trace_checkpoint_created(&mut self, ip: u16) {
        self.checkpoint_ips.push(ip);
    }

    fn trace_backtrack(&mut self) {
        let created_at = self
            .checkpoint_ips
            .pop()
            .expect("backtrack without checkpoint");
        let line = format!(
            "  {:0sw$} {}",
            created_at,
            trace::BACKTRACK.format(),
            sw = self.step_width
        );
        self.lines.push(line);
    }

    fn trace_enter_entrypoint(&mut self, target_ip: u16) {
        let name = self.entrypoint_name(target_ip).to_string();
        self.push_def_label(&name);
        self.definition_stack.push(name);
    }

    fn trace_enter_preamble(&mut self) {
        const PREAMBLE_NAME: &str = "_ObjWrap";
        self.push_def_label(PREAMBLE_NAME);
        self.definition_stack.push(PREAMBLE_NAME.to_string());
    }
}

/// Format match successors for instruction line.
fn format_match_successors(m: &MatchView<'_>) -> String {
    if m.is_terminal() {
        "◼".to_string()
    } else if m.succ_count() == 1 {
        format!("{:02}", m.successor(0).get())
    } else {
        let succs: Vec<_> = m.successors().map(|s| format!("{:02}", s.get())).collect();
        succs.join(", ")
    }
}
